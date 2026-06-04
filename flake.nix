{
  description = "Cala";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs = {
        nixpkgs.follows = "nixpkgs";
      };
    };
    advisory-db = {
      url = "github:rustsec/advisory-db";
      flake = false;
    };
    crane.url = "github:ipetkov/crane";
    process-compose-flake.url = "github:Platonic-Systems/process-compose-flake";
  };
  outputs = {
    self,
    nixpkgs,
    flake-utils,
    rust-overlay,
    advisory-db,
    crane,
    process-compose-flake,
  }:
    flake-utils.lib.eachDefaultSystem
    (system: let
      overlays = [
        (import rust-overlay)
      ];
      pkgs = import nixpkgs {
        inherit system overlays;
      };
      rustVersion = pkgs.pkgsBuildHost.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
      rustToolchain = rustVersion.override {
        extensions = ["rust-analyzer" "rust-src" "clippy"];
      };
      craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

      rustSource = pkgs.lib.cleanSourceWith {
        src = craneLib.path ./.;
        filter = path: type:
          craneLib.filterCargoSources path type
          || pkgs.lib.hasInfix "/.sqlx/" path
          || pkgs.lib.hasSuffix ".lalrpop" path
          || pkgs.lib.hasSuffix ".sql" path
          || (builtins.match ".*deny\.toml$" path != null);
      };
      commonArgs = {
        src = rustSource;
        SQLX_OFFLINE = "true";
      };
      cargoArtifacts = craneLib.buildDepsOnly commonArgs;

      nativeBuildInputs = with pkgs; [
        ytt
        gnuplot
        rustToolchain
        alejandra
        sqlx-cli
        cargo-nextest
        cargo-audit
        cargo-watch
        cargo-deny
        samply
        bacon
        postgresql
        process-compose
        bc
        jq
      ];

      pgPort = 5432;
      pgUser = "user";
      pgPassword = "password";
      pgDatabase = "pg";

      # ── Per-worktree dev env ───────────────────────────────────────────
      # Derives a base port from a CRC32 of the checkout path so multiple
      # cala checkouts can run their dev Postgres in parallel.
      devEnv = pkgs.writeShellApplication {
        name = "cala-dev-env";
        runtimeInputs = [pkgs.coreutils];
        text = ''
          cwd_hash=$(printf '%s' "$PWD" | cksum | cut -d' ' -f1)
          base_port=$((20000 + (cwd_hash % 120) * 100))

          PGPORT="''${PGPORT:-$base_port}"
          PC_PORT_NUM="''${PC_PORT_NUM:-$((base_port + 1))}"
          DATABASE_URL="''${DATABASE_URL:-postgres://${pgUser}:${pgPassword}@127.0.0.1:$PGPORT/${pgDatabase}?sslmode=disable}"

          emit() { printf 'export %s=%q\n' "$1" "$2"; }
          emit PGPORT "$PGPORT"
          emit PC_PORT_NUM "$PC_PORT_NUM"
          emit PGHOST 127.0.0.1
          emit PGUSER ${pgUser}
          emit PGPASSWORD ${pgPassword}
          emit PGDATABASE ${pgDatabase}
          emit DATABASE_URL "$DATABASE_URL"
          emit PG_CON "$DATABASE_URL"
        '';
      };

      # ── Postgres start helper ──────────────────────────────────────────
      pg-start = pkgs.writeShellApplication {
        name = "pg-start";
        runtimeInputs =
          [pkgs.postgresql pkgs.coreutils]
          ++ pkgs.lib.optionals pkgs.stdenv.isLinux [pkgs.util-linux];
        text = ''
          NAME="$1" PORT="$2" PGUSER="$3" DB="$4"
          PGDATA="$PWD/.nix-deps/$NAME"

          # PostgreSQL refuses to run as root. When running as root (e.g. CI),
          # drop privileges to _pgdev (UID 70) via setpriv.
          PG_UID=70
          PG_GID=70
          IS_ROOT=false
          if [ "$(id -u)" = "0" ]; then
            IS_ROOT=true
          fi

          run_pg() {
            if [ "$IS_ROOT" = "true" ]; then
              setpriv --reuid=$PG_UID --regid=$PG_GID --clear-groups -- "$@"
            else
              "$@"
            fi
          }

          mkdir -p "$PWD/.nix-deps"

          if [ ! -f "$PGDATA/PG_VERSION" ]; then
            echo "[$NAME] Initializing data directory at $PGDATA..."
            mkdir -p "$PGDATA"
            if [ "$IS_ROOT" = "true" ]; then chown -R $PG_UID:$PG_GID "$PGDATA"; fi
            run_pg initdb -D "$PGDATA" --username="$PGUSER" --auth=trust --no-locale -E UTF8
            {
              echo "port = $PORT"
              echo "unix_socket_directories = '/tmp'"
              echo "listen_addresses = '127.0.0.1'"
            } >> "$PGDATA/postgresql.conf"
          else
            if [ "$IS_ROOT" = "true" ]; then chown -R $PG_UID:$PG_GID "$PGDATA"; fi
          fi

          if [ -f "$PGDATA/postmaster.pid" ]; then
            run_pg pg_ctl -D "$PGDATA" stop -m immediate 2>/dev/null || rm -f "$PGDATA/postmaster.pid"
          fi

          run_pg postgres -D "$PGDATA" -p "$PORT" -k /tmp &
          PG_PID=$!
          trap 'kill $PG_PID 2>/dev/null; wait $PG_PID 2>/dev/null' EXIT

          while ! pg_isready -p "$PORT" -U "$PGUSER" -h 127.0.0.1 -q 2>/dev/null; do
            kill -0 "$PG_PID" 2>/dev/null || {
              echo "[$NAME] ERROR: postgres exited during startup (port $PORT)" >&2
              exit 1
            }
            sleep 0.1
          done

          if [ "$DB" != "$PGUSER" ]; then
            createdb -p "$PORT" -U "$PGUSER" -h 127.0.0.1 "$DB" 2>/dev/null || {
              if psql -p "$PORT" -U "$PGUSER" -h 127.0.0.1 -lqt | cut -d \| -f 1 | grep -qw "$DB"; then
                echo "[$NAME] Database '$DB' already exists"
              else
                echo "[$NAME] ERROR: Failed to create database '$DB'" >&2
                exit 1
              fi
            }
          fi

          echo "[$NAME] Ready on port $PORT (database: $DB)"
          wait $PG_PID
        '';
      };

      setupDbDev = pkgs.writeShellApplication {
        name = "setup-db-dev";
        runtimeInputs = [pkgs.sqlx-cli pkgs.coreutils];
        text = ''
          eval "$(${devEnv}/bin/cala-dev-env)"
          cd cala-ledger
          exec sqlx migrate run
        '';
      };

      # ── process-compose: core-pg ───────────────────────────────────────
      pcLib = import process-compose-flake.lib {inherit pkgs;};

      mkPg = {
        name,
        port,
        user,
        db,
      }: {
        command = "${
          pkgs.writeShellApplication {
            name = "start-${name}";
            runtimeInputs = [pg-start];
            text = ''
              exec pg-start ${name} "''${PGPORT:-${toString port}}" ${user} ${db}
            '';
          }
        }/bin/start-${name}";
        readiness_probe = {
          exec.command = "${
            pkgs.writeShellApplication {
              name = "ready-${name}";
              runtimeInputs = [pkgs.postgresql];
              text = ''
                exec psql -p "''${PGPORT:-${toString port}}" -U ${user} -h 127.0.0.1 -d ${db} -c 'SELECT 1' -t -q
              '';
            }
          }/bin/ready-${name}";
          initial_delay_seconds = 1;
          period_seconds = 1;
          failure_threshold = 60;
        };
        shutdown = {
          signal = 2;
          timeout_seconds = 10;
        };
      };

      # Only long-running services live in process-compose. Migrations are run
      # synchronously by callers (Makefile start-deps / nextest-runner) after
      # core-pg is healthy, so tests never start mid-migration.
      baseProcesses = {
        core-pg = mkPg {
          name = "core-pg";
          port = pgPort;
          user = pgUser;
          db = pgDatabase;
        };
      };

      nix-deps-base = pcLib.makeProcessCompose {
        name = "nix-deps-base";
        modules = [
          {
            settings = {
              log_level = "info";
              log_location = ".nix-deps/process-compose.log";
              processes = baseProcesses;
            };
          }
        ];
      };

      # Bring up process-compose Postgres, wait for readiness, run migrations.
      # Inlined into each runner (not a separate script) so the XDG_CONFIG_HOME
      # export and the `down` in the cleanup trap share one shell.
      depsUp = ''
        mkdir -p .nix-deps

        # process-compose reads/writes config under XDG_CONFIG_HOME; the
        # stripped CI image has no /root/.config. Point it at a writable dir.
        export XDG_CONFIG_HOME="''${XDG_CONFIG_HOME:-$PWD/.nix-deps/config}"
        mkdir -p "$XDG_CONFIG_HOME"

        # Create _pgdev user (UID 70) for pg-start's setpriv drop when running as root.
        if [ "$(id -u)" = "0" ]; then
          if ! getent passwd 70 >/dev/null 2>&1; then
            echo "_pgdev:x:70:70::/tmp:/bin/false" >> /etc/passwd
            echo "_pgdev:x:70:" >> /etc/group
          fi
        fi

        echo "Starting PostgreSQL via process-compose..."
        ${nix-deps-base}/bin/nix-deps-base up -D

        # Bounded readiness wait. `is-ready --wait` has no timeout and hangs on failure
        for i in $(seq 1 60); do
          if ${nix-deps-base}/bin/nix-deps-base project is-ready 2>/dev/null; then
            echo "Services ready after ''${i}x5s"
            break
          fi
          if [ "$i" = "60" ]; then
            echo "ERROR: services not ready after 5 minutes" >&2
            ${nix-deps-base}/bin/nix-deps-base process list || true
            exit 1
          fi
          sleep 5
        done

        echo "Running database migrations..."
        ${setupDbDev}/bin/setup-db-dev
      '';

      perf-runner = pkgs.writeShellScriptBin "perf-runner" ''
        set -e

        export PATH="${pkgs.lib.makeBinPath [
          pkgs.sqlx-cli
          pkgs.coreutils
          pkgs.gnumake
          pkgs.gnuplot
          pkgs.jq
          pkgs.bc
          pkgs.gnused
          pkgs.gnugrep
          pkgs.findutils
          pkgs.postgresql
          rustToolchain
          pkgs.stdenv.cc
        ]}:$PATH"

        export SQLX_OFFLINE="true"
        eval "$(${devEnv}/bin/cala-dev-env)"

        if [ $# -eq 0 ]; then
          echo "Usage: perf-runner <output-file>"
          exit 1
        fi
        OUTPUT_FILE="$1"

        cleanup() {
          echo "Stopping deps..."
          ${nix-deps-base}/bin/nix-deps-base down 2>/dev/null || true
        }
        trap cleanup EXIT

        ${depsUp}

        echo "Running perf DB setup..."
        ${pkgs.postgresql}/bin/psql "$DATABASE_URL" -f ./cala-perf/pg-tools/setup.sql

        echo "Running benchmarks..."
        cargo bench -p cala-perf

        echo "Running load tests..."
        cargo run -p cala-perf 2>&1 | tee load-output.txt
        load_output=$(cat load-output.txt)

        {
        echo "## Cala Performance Benchmark Results (non-representative)"
        echo "### Criterion Benchmark Results (single-threaded)"
        echo ""
        echo "| Benchmark | Time per Run | Throughput | % vs Baseline |"
        echo "|-----------|--------------|------------|---------------|"

        baseline_time=""
        for json_file in target/criterion/*/new/estimates.json; do
            if [[ -f "$json_file" ]]; then
                bench_name=$(basename "$(dirname "$(dirname "$json_file")")")
                time_ns=$(${pkgs.jq}/bin/jq -r '.mean.point_estimate' "$json_file")
                if [[ -n "$time_ns" && "$time_ns" != "null" ]]; then
                    time_ms=$(echo "scale=3; $time_ns / 1000000" | ${pkgs.bc}/bin/bc -l)
                    time_display="''${time_ms}ms"
                    tx_per_sec=$(echo "scale=0; 1000000000 / $time_ns" | ${pkgs.bc}/bin/bc -l)
                    if [[ -z "$baseline_time" ]]; then
                        baseline_time=$time_ns
                        perc_diff="0 (baseline)"
                    else
                        perc_diff=$(echo "scale=2; ($time_ns - $baseline_time) / $baseline_time * 100" | ${pkgs.bc}/bin/bc -l | xargs printf "%.1f")
                        if (( $(echo "$perc_diff >= 0" | ${pkgs.bc}/bin/bc -l) )); then
                            perc_diff="-''${perc_diff}%"
                        else
                            perc_diff="+''${perc_diff#-}%"
                        fi
                    fi
                    echo "| ''${bench_name#* } | $time_display | ''${tx_per_sec} tx/s | $perc_diff |"
                fi
            fi
        done

        echo ""
        echo "### Load Testing Results (parallel-execution)"
        echo ""
        echo "$load_output" | ${pkgs.gnused}/bin/sed -n '/PERFORMANCE SUMMARY TABLE/,/All performance tests completed!/p' | ${pkgs.gnused}/bin/sed '$d' | ${pkgs.gnused}/bin/sed '1,2d'
        echo "---"
        echo ""
        echo "**Note**: Performance results may vary based on system resources and database state."
        } > "$OUTPUT_FILE"

        echo "Performance report generated: $OUTPUT_FILE"
      '';

      nextest-runner = pkgs.writeShellScriptBin "nextest-runner" ''
        set -e

        export PATH="${pkgs.lib.makeBinPath [
          pkgs.sqlx-cli
          pkgs.cargo-nextest
          pkgs.coreutils
          pkgs.gnumake
          rustToolchain
          pkgs.stdenv.cc
        ]}:$PATH"

        export SQLX_OFFLINE="true"
        eval "$(${devEnv}/bin/cala-dev-env)"

        cleanup() {
          echo "Stopping deps..."
          ${nix-deps-base}/bin/nix-deps-base down 2>/dev/null || true
        }
        trap cleanup EXIT

        ${depsUp}

        echo "Running nextest..."
        cargo nextest run --verbose --locked --workspace

        echo "Running doc tests..."
        cargo test --doc --workspace

        echo "Building docs..."
        cargo doc --no-deps --workspace

        echo "Tests completed successfully!"
      '';
    in
      with pkgs; {
        packages = {
          nextest = nextest-runner;
          perf = perf-runner;
          setup-db-dev = setupDbDev;
          dev-env = devEnv;
          inherit nix-deps-base;
        };

        apps.setup-db-dev = flake-utils.lib.mkApp {
          drv = setupDbDev;
          name = "setup-db-dev";
        };

        apps.dev-env = flake-utils.lib.mkApp {
          drv = devEnv;
          name = "cala-dev-env";
        };

        checks = {
          workspace-fmt = craneLib.cargoFmt commonArgs;
          workspace-clippy = craneLib.cargoClippy (commonArgs
            // {
              inherit cargoArtifacts;
              cargoClippyExtraArgs = "--all-features -- --deny warnings";
            });
          workspace-audit = craneLib.cargoAudit {
            inherit advisory-db;
            src = rustSource;
          };
          workspace-deny = craneLib.cargoDeny {
            src = rustSource;
          };
          check-fmt = stdenv.mkDerivation {
            name = "check-fmt";
            src = ./.;
            nativeBuildInputs = [alejandra];
            dontBuild = true;
            doCheck = true;
            checkPhase = ''
              alejandra -qc .
            '';
            installPhase = ''
              mkdir -p $out
            '';
          };
        };

        devShells.default = mkShell {
          inherit nativeBuildInputs;
          shellHook = ''
            eval "$(${devEnv}/bin/cala-dev-env)"
          '';
        };

        formatter = alejandra;
      });
}
