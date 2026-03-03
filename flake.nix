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
  };
  outputs = {
    self,
    nixpkgs,
    flake-utils,
    rust-overlay,
    advisory-db,
    crane,
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
        docker-compose
        wait4x
        bc
        jq
        podman
        podman-compose
      ];
      devEnvVars = rec {
        OTEL_EXPORTER_OTLP_ENDPOINT = http://localhost:4317;
        DATABASE_URL = "postgres://user:password@127.0.0.1:5432/pg?sslmode=disable";
        PG_CON = "${DATABASE_URL}";
      };
      podman-runner = pkgs.callPackage ./nix/podman-runner.nix {};

      perf-runner = pkgs.writeShellScriptBin "perf-runner" ''
        set -e

        export PATH="${pkgs.lib.makeBinPath [
          podman-runner.podman-compose-runner
          pkgs.wait4x
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
        export DATABASE_URL="${devEnvVars.DATABASE_URL}"
        export PG_CON="${devEnvVars.PG_CON}"

        if [ $# -eq 0 ]; then
          echo "Usage: perf-runner <output-file>"
          exit 1
        fi
        OUTPUT_FILE="$1"

        cleanup() {
          echo "Stopping deps..."
          ${podman-runner.podman-compose-runner}/bin/podman-compose-runner down || true
        }
        trap cleanup EXIT

        echo "Starting dependencies..."
        ${podman-runner.podman-compose-runner}/bin/podman-compose-runner up -d integration-deps

        echo "Waiting for PostgreSQL to be ready..."
        ${pkgs.wait4x}/bin/wait4x postgresql "$DATABASE_URL" --timeout 120s

        echo "Running database migrations..."
        for i in $(seq 1 30); do
          if (cd cala-ledger && ${pkgs.sqlx-cli}/bin/sqlx migrate run 2>/dev/null); then
            echo "Migrations complete"
            break
          fi
          echo "Attempt $i: Database not ready, waiting..."
          sleep 1
          if [ "$i" -eq 30 ]; then
            echo "Database failed to become ready after 30 attempts"
            cd cala-ledger && ${pkgs.sqlx-cli}/bin/sqlx migrate run
          fi
        done

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
          podman-runner.podman-compose-runner
          pkgs.wait4x
          pkgs.sqlx-cli
          pkgs.cargo-nextest
          pkgs.coreutils
          pkgs.gnumake
          rustToolchain
          pkgs.stdenv.cc
        ]}:$PATH"

        export SQLX_OFFLINE="true"
        export DATABASE_URL="${devEnvVars.DATABASE_URL}"
        export PG_CON="${devEnvVars.PG_CON}"

        cleanup() {
          echo "Stopping deps..."
          ${podman-runner.podman-compose-runner}/bin/podman-compose-runner down || true
        }

        trap cleanup EXIT

        echo "Starting dependencies..."
        ${podman-runner.podman-compose-runner}/bin/podman-compose-runner up -d integration-deps

        echo "Waiting for PostgreSQL to be ready..."
        ${pkgs.wait4x}/bin/wait4x postgresql "$DATABASE_URL" --timeout 120s

        echo "Running database migrations..."
        for i in $(seq 1 30); do
          if (cd cala-ledger && ${pkgs.sqlx-cli}/bin/sqlx migrate run 2>/dev/null); then
            echo "Migrations complete"
            break
          fi
          echo "Attempt $i: Database not ready, waiting..."
          sleep 1
          if [ "$i" -eq 30 ]; then
            echo "Database failed to become ready after 30 attempts"
            cd cala-ledger && ${pkgs.sqlx-cli}/bin/sqlx migrate run
          fi
        done

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

        devShells.default = mkShell (devEnvVars
          // {
            inherit nativeBuildInputs;
          });

        formatter = alejandra;
      });
}
