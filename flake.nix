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

      cala-server-debug = craneLib.buildPackage {
        src = rustSource;
        strictDeps = true;
        SQLX_OFFLINE = true;
        pname = "cala-server-debug";
        cargoExtraArgs = "-p cala-server";
        inherit (craneLib.crateNameFromCargoToml {cargoToml = ./cala-server/Cargo.toml;}) version;
        doCheck = false;
      };

      nativeBuildInputs = with pkgs; [
        wait4x
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
        bats
        bc
        jq
        ytt
        podman
        podman-compose
        curl
        procps
      ];
      devEnvVars = rec {
        OTEL_EXPORTER_OTLP_ENDPOINT = http://localhost:4317;
        DATABASE_URL = "postgres://user:password@127.0.0.1:5432/pg?sslmode=disable";
        PG_CON = "${DATABASE_URL}";
      };
      podman-runner = pkgs.callPackage ./nix/podman-runner.nix {};

      bats-runner = let
        binPath = pkgs.lib.makeBinPath [
          podman-runner.podman-compose-runner
          pkgs.podman
          pkgs.wait4x
          pkgs.bats
          pkgs.jq
          pkgs.curl
          pkgs.procps
          pkgs.coreutils
          pkgs.gnugrep
          pkgs.gnused
          pkgs.findutils
          pkgs.sqlx-cli
          cala-server-debug
        ];
      in
        pkgs.symlinkJoin {
          name = "bats-runner";
          paths = [
            podman-runner.podman-compose-runner
            pkgs.podman
            pkgs.wait4x
            pkgs.bats
            pkgs.jq
            pkgs.curl
            pkgs.procps
            pkgs.coreutils
            pkgs.gnugrep
            pkgs.gnused
            pkgs.findutils
            pkgs.sqlx-cli
            cala-server-debug
          ];
          postBuild = ''
            mkdir -p $out/bin
            cat > $out/bin/bats-runner << 'EOF'
            #!${pkgs.bash}/bin/bash
            set -euo pipefail

            # Add all tools to PATH
            export PATH="${binPath}:$PATH"

            # Set environment variables
            export CALA_BIN="${cala-server-debug}/bin/cala-server"
            export PG_CON="${devEnvVars.PG_CON}"
            export DATABASE_URL="${devEnvVars.DATABASE_URL}"
            export DOCKER_ENGINE=podman

            # Function to cleanup on exit
            cleanup() {
              echo "Stopping podman-compose..."
              podman-compose-runner -f docker-compose.yml down || true
            }

            # Register cleanup function
            trap cleanup EXIT

            echo "Starting dependencies with podman-compose..."
            podman-compose-runner -f docker-compose.yml up -d integration-deps

            echo "Waiting for PostgreSQL to be ready..."
            wait4x postgresql "$PG_CON" --timeout 120s

            echo "Running database migrations..."
            for i in $(seq 1 30); do
              if (cd cala-ledger && sqlx migrate run 2>/dev/null); then
                echo "Migrations complete"
                break
              fi
              echo "Attempt $i: Database not ready, waiting..."
              sleep 1
              if [ "$i" -eq 30 ]; then
                echo "Database failed to become ready after 30 attempts"
                cd cala-ledger && sqlx migrate run
              fi
            done

            # Set TERM for CI environments
            export TERM="''${TERM:-dumb}"
            echo "Running bats tests with CALA_BIN=$CALA_BIN..."
            bats -t bats

            echo "Tests completed successfully!"
            EOF
            chmod +x $out/bin/bats-runner
          '';
        };
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
          cala-server-debug = cala-server-debug;
          bats-runner = bats-runner;
          nextest = nextest-runner;
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

        apps.bats = flake-utils.lib.mkApp {
          drv = self.packages.${system}.bats-runner;
          name = "bats-runner";
        };

        formatter = alejandra;
      });
}
