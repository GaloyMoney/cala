{
  description = "Cala";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    crane.url = "github:ipetkov/crane";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs = {
        nixpkgs.follows = "nixpkgs";
      };
    };
  };
  outputs = {
    self,
    nixpkgs,
    flake-utils,
    crane,
    rust-overlay,
  }:
    flake-utils.lib.eachDefaultSystem
    (system: let
      overlays = [
        (self: super: {
          nodejs = super.nodejs_20;
        })
        (import rust-overlay)
      ];
      pkgs = import nixpkgs {
        inherit system overlays;
      };
      
      rustVersion = pkgs.pkgsBuildHost.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
      rustToolchain = rustVersion.override {
        extensions = ["rust-analyzer" "rust-src" "rustfmt" "clippy"];
      };
      
      craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

      rustSource = pkgs.lib.cleanSourceWith {
        src = ./.;
        filter = path: type:
          craneLib.filterCargoSources path type
          || pkgs.lib.hasInfix "/migrations/" path
          || pkgs.lib.hasInfix "/proto/" path
          || pkgs.lib.hasInfix "/.sqlx/" path
          || !(pkgs.lib.hasInfix "/target/" path);
      };

      sqlxSource = pkgs.lib.cleanSourceWith {
        src = ./.;
        filter = path: type:
          (builtins.match ".*/.sqlx/.*\\.json$" path != null);
      };
      
      
      commonArgs = {
        src = rustSource;
        strictDeps = true;
        cargoToml = ./Cargo.toml;
        cargoLock = ./Cargo.lock;
        version = "0.1.0";
        pname = "cala";
        
        buildInputs = with pkgs; [
          protobuf
          postgresql
        ] ++ lib.optionals pkgs.stdenv.isDarwin [
          darwin.apple_sdk.frameworks.SystemConfiguration
        ];
        
        nativeBuildInputs = with pkgs; [
          protobuf
          pkg-config
          cacert
          gitMinimal
          coreutils
          sqlx-cli
          lalrpop
        ];
        
        SQLX_OFFLINE = true;
        PROTOC = "${pkgs.protobuf}/bin/protoc";
        PROTOC_INCLUDE = "${pkgs.protobuf}/include";
        
        preBuildPhases = ["generatePhase" "copySqlxPhase"];
        generatePhase = ''
          # Generate LALRPOP parser
          if [ -f cala-cel-parser/src/cel.lalrpop ]; then
            cd cala-cel-parser
            ${pkgs.lalrpop}/bin/lalrpop src/cel.lalrpop
            cd ..
          fi
        '';

        copySqlxPhase = ''
          # Copy SQLx files
          mkdir -p lib/es-entity/.sqlx
          if [ -d "${sqlxSource}/lib/es-entity/.sqlx" ]; then
            cp -r ${sqlxSource}/lib/es-entity/.sqlx/* lib/es-entity/.sqlx/ || true
          fi

          mkdir -p cala-server/.sqlx
          if [ -d "${sqlxSource}/cala-server/.sqlx" ]; then
            cp -r ${sqlxSource}/cala-server/.sqlx/* cala-server/.sqlx/ || true
          fi

          mkdir -p cala-ledger/.sqlx
          if [ -d "${sqlxSource}/cala-ledger/.sqlx" ]; then
            cp -r ${sqlxSource}/cala-ledger/.sqlx/* cala-ledger/.sqlx/ || true
          fi
        '';

        configurePhase = ''
          export CARGO_NET_GIT_FETCH_WITH_CLI=true
          export PROTOC="${pkgs.protobuf}/bin/protoc"
          export PATH="${pkgs.protobuf}/bin:${pkgs.gitMinimal}/bin:${pkgs.coreutils}/bin:$PATH"
          export SSL_CERT_FILE="${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
          export CARGO_HTTP_CAINFO="$SSL_CERT_FILE"
          export GIT_SSL_CAINFO="$SSL_CERT_FILE"
        '';
      };
      
      cargoArtifacts = craneLib.buildDepsOnly (commonArgs // {
        pname = "cala-deps";
        version = "0.1.0";
      });

      cala = craneLib.buildPackage (commonArgs // {
        inherit cargoArtifacts;
        pname = "cala";
        doCheck = false;
      });

      cala-server = craneLib.buildPackage (commonArgs // {
        inherit cargoArtifacts;
        pname = "cala-server";
        doCheck = false;
        cargoExtraArgs = "-p cala-server";
      });

      cala-ledger = craneLib.buildPackage (commonArgs // {
        inherit cargoArtifacts;
        pname = "cala-ledger";
        doCheck = false;
        cargoExtraArgs = "-p cala-ledger";
      });

      write-sdl = craneLib.buildPackage (commonArgs // {
        inherit cargoArtifacts;
        pname = "write-sdl";
        doCheck = false;
        cargoExtraArgs = "--bin write_sdl";
      });

      cala-ledger-outbox-client = craneLib.buildPackage (commonArgs // {
        inherit cargoArtifacts;
        pname = "cala-ledger-outbox-client";
        doCheck = false;
        cargoExtraArgs = "-p cala-ledger-outbox-client";
      });

      cala-cel-parser = craneLib.buildPackage (commonArgs // {
        inherit cargoArtifacts;
        pname = "cala-cel-parser";
        doCheck = false;
        cargoExtraArgs = "-p cala-cel-parser";
      });

      cala-cel-interpreter = craneLib.buildPackage (commonArgs // {
        inherit cargoArtifacts;
        pname = "cala-cel-interpreter";
        doCheck = false;
        cargoExtraArgs = "-p cala-cel-interpreter";
      });

      cala-nodejs = craneLib.buildPackage (commonArgs // {
        inherit cargoArtifacts;
        pname = "cala-nodejs";
        doCheck = false;
        cargoExtraArgs = "-p galoymoney_cala-ledger";
        
        nativeBuildInputs = commonArgs.nativeBuildInputs ++ (with pkgs; [
          napi-rs-cli
          nodejs
        ]);
      });

      cala-ledger-example-rust = craneLib.buildPackage (commonArgs // {
        inherit cargoArtifacts;
        pname = "cala-ledger-example-rust";
        doCheck = false;
        cargoExtraArgs = "--bin cala-ledger-example-rust";
      });

    checkCode = pkgs.writeShellScriptBin "check-code" ''
      set -euo pipefail
      export PATH="${pkgs.git}/bin:${pkgs.cargo}/bin:$PATH"
      
      # Run write-sdl first to generate schema
      ${write-sdl}/bin/write_sdl > cala-server/schema.graphql
      
      # Check if schema has changed
      if ! git diff --exit-code cala-server/schema.graphql; then
        echo "Schema file has changed. Please commit the changes."
        exit 1
      fi
      
      # Format check - exclude generated parser file by checking specific packages
      SQLX_OFFLINE=true cargo fmt --check \
        --package cala-ledger \
        --package cala-ledger-outbox-client \
        --package cala-ledger-core-types \
        --package cala-server \
        --package cala-tracing \
        --package galoymoney_cala-ledger \
        --package cala-cel-interpreter \
        --package es-entity \
        --package es-entity-macros \
        --package sim-time \
        --package cala-ledger-example-rust
      
      # Basic check
      SQLX_OFFLINE=true cargo check
      
      # Clippy checks with specific packages and features
      SQLX_OFFLINE=true cargo clippy --package es-entity --all-features
      SQLX_OFFLINE=true cargo clippy --package cala-server --features=
      SQLX_OFFLINE=true cargo clippy --package cala-ledger --features="import,graphql"
      SQLX_OFFLINE=true cargo clippy --package cala-ledger-core-types --features="graphql"
      SQLX_OFFLINE=true cargo clippy --workspace --exclude es-entity --exclude cala-server --exclude cala-ledger --exclude cala-ledger-core-types
      
      # Security audit
      SQLX_OFFLINE=true cargo audit
      
      # Deny check
      SQLX_OFFLINE=true cargo deny check
    '';
      


      testInCi = pkgs.writeShellScriptBin "test-in-ci" ''
      set -euo pipefail
      export PATH="${pkgs.cargo}/bin:$PATH"
      
      # Run cargo test
      SQLX_OFFLINE=true cargo test
      
      # Run integration tests if they exist
      if [ -d "tests" ]; then
        echo "Running integration tests..."
        SQLX_OFFLINE=true cargo test --test "*"
      fi
    '';



      nativeBuildInputs = with pkgs;
        [
          rustToolchain
          alejandra
          sqlx-cli
          cargo-nextest
          cargo-audit
          cargo-watch
          cargo-deny
          bacon
          postgresql
          docker-compose
          bats
          jq
          napi-rs-cli
          yarn
          nodejs
          typescript
          ytt
          podman
          podman-compose
          curl
          lalrpop
        ]
        ++ lib.optionals pkgs.stdenv.isDarwin [
          darwin.apple_sdk.frameworks.SystemConfiguration
        ];

      devEnvVars = rec {
        OTEL_EXPORTER_OTLP_ENDPOINT = "http://localhost:4317";
        PGDATABASE = "pg";
        PGUSER = "user";
        PGPASSWORD = "password";
        PGHOST = "127.0.0.1";
        DATABASE_URL = "postgres://${PGUSER}:${PGPASSWORD}@${PGHOST}:5432/pg";
        PG_CON = "${DATABASE_URL}";
        SQLX_OFFLINE = "true";
      };
    in
      with pkgs; {
        packages = {
          default = cala;
          inherit 
            cala 
            cala-server 
            cala-ledger 
            cala-ledger-outbox-client
            cala-cel-parser
            cala-cel-interpreter
            cala-nodejs
            write-sdl
            cala-ledger-example-rust
            checkCode 
            testInCi;
        };
        
        checks = {
          inherit cala cala-server cala-ledger cala-ledger-outbox-client cala-cel-parser cala-cel-interpreter cala-nodejs;
        };
        
        apps = {
          default = flake-utils.lib.mkApp {
            drv = cala-server;
            name = "cala-server";
          };
          
          cala-server = flake-utils.lib.mkApp {
            drv = cala-server;
            name = "cala-server";
          };
          
          cala-ledger = flake-utils.lib.mkApp {
            drv = cala-ledger;
            name = "cala-ledger";
          };

          write-sdl = flake-utils.lib.mkApp {
            drv = write-sdl;
            name = "write_sdl";
          };

          cala-ledger-example-rust = flake-utils.lib.mkApp {
            drv = cala-ledger-example-rust;
            name = "cala-ledger-example-rust";
          };

          cala-ledger-outbox-client = flake-utils.lib.mkApp {
            drv = cala-ledger-outbox-client;
            name = "cala-ledger-outbox-client";
          };

          cala-cel-parser = flake-utils.lib.mkApp {
            drv = cala-cel-parser;
            name = "cala-cel-parser";
          };

          cala-cel-interpreter = flake-utils.lib.mkApp {
            drv = cala-cel-interpreter;
            name = "cala-cel-interpreter";
          };
        };

        devShells.default = mkShell (devEnvVars
          // {
            inherit nativeBuildInputs;
            shellHook = ''
              # Add any shell initialization here
              echo "Welcome to Cala development environment!"
              echo "Database URL: $DATABASE_URL"
            '';
          });

        formatter = alejandra;
      });
}
