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
    crane.url = "github:ipetkov/crane";
  };
  outputs = {
    self,
    nixpkgs,
    flake-utils,
    rust-overlay,
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
        extensions = ["rust-analyzer" "rust-src"];
      };
      craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

      rustSource = pkgs.lib.cleanSourceWith {
        src = ./.;
        filter = path: type:
          craneLib.filterCargoSources path type
          || pkgs.lib.hasInfix "/.sqlx/" path
          || pkgs.lib.hasSuffix ".lalrpop" path
          || pkgs.lib.hasSuffix ".sql" path;
      };

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
    in
      with pkgs; {
        devShells.default = mkShell (devEnvVars
          // {
            inherit nativeBuildInputs;
          });

        packages.cala-server-debug = cala-server-debug;
        packages.bats-runner = bats-runner;

        apps.bats = flake-utils.lib.mkApp {
          drv = self.packages.${system}.bats-runner;
          name = "bats-runner";
        };

        formatter = alejandra;
      });
}
