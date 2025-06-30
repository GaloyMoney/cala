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
          || pkgs.lib.hasInfix "/.sqlx/" path;
      };
      
      # Source filter for format checking - excludes generated files
      rustSourceFormatCheck = pkgs.lib.cleanSourceWith {
        src = ./.;
        filter = path: type:
          craneLib.filterCargoSources path type
          && !pkgs.lib.hasInfix "/target/" path
          && !pkgs.lib.hasInfix "parser.rs" path  # Exclude generated LALRPOP files
          && !pkgs.lib.hasInfix ".lalrpop" path;   # Exclude LALRPOP grammar files
      };
      
      commonArgs = {
        src = rustSource;
        strictDeps = true;
        cargoToml = ./Cargo.toml;
        cargoLock = ./Cargo.lock;
        
        # Explicit version to silence crane warnings
        version = "0.1.0";
        
        buildInputs = with pkgs; [
          protobuf
        ] ++ lib.optionals pkgs.stdenv.isDarwin [
          darwin.apple_sdk.frameworks.SystemConfiguration
        ];
        
        nativeBuildInputs = with pkgs; [
          protobuf
          pkg-config
          cacert
          gitMinimal
          coreutils
        ];
        
        SQLX_OFFLINE = true;
        PROTOC = "${pkgs.protobuf}/bin/protoc";
        PROTOC_INCLUDE = "${pkgs.protobuf}/include";
      };
      
      cargoArtifacts = craneLib.buildDepsOnly (commonArgs // {
        pname = "cala-deps";
        
        configurePhase = ''
          export CARGO_NET_GIT_FETCH_WITH_CLI=true
          export PROTOC="${pkgs.protobuf}/bin/protoc"
          export PATH="${pkgs.protobuf}/bin:${pkgs.gitMinimal}/bin:${pkgs.coreutils}/bin:$PATH"
          export SSL_CERT_FILE="${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
          export CARGO_HTTP_CAINFO="$SSL_CERT_FILE"
          export GIT_SSL_CAINFO="$SSL_CERT_FILE"
        '';
      });
      
      # Main cala package
      cala = craneLib.buildPackage (commonArgs // {
        inherit cargoArtifacts;
        pname = "cala";
        doCheck = false;
        
        configurePhase = ''
          export CARGO_NET_GIT_FETCH_WITH_CLI=true
          export PROTOC="${pkgs.protobuf}/bin/protoc"
          export PATH="${pkgs.protobuf}/bin:${pkgs.gitMinimal}/bin:${pkgs.coreutils}/bin:$PATH"
          export SSL_CERT_FILE="${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
          export CARGO_HTTP_CAINFO="$SSL_CERT_FILE"
          export GIT_SSL_CAINFO="$SSL_CERT_FILE"
        '';
      });

      # Cala server package
      cala-server = craneLib.buildPackage (commonArgs // {
        inherit cargoArtifacts;
        pname = "cala-server";
        doCheck = false;
        cargoExtraArgs = "-p cala-server";
        
        configurePhase = ''
          export CARGO_NET_GIT_FETCH_WITH_CLI=true
          export PROTOC="${pkgs.protobuf}/bin/protoc"
          export PATH="${pkgs.protobuf}/bin:${pkgs.gitMinimal}/bin:${pkgs.coreutils}/bin:$PATH"
          export SSL_CERT_FILE="${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
          export CARGO_HTTP_CAINFO="$SSL_CERT_FILE"
          export GIT_SSL_CAINFO="$SSL_CERT_FILE"
        '';
      });

      # Cala ledger package
      cala-ledger = craneLib.buildPackage (commonArgs // {
        inherit cargoArtifacts;
        pname = "cala-ledger";
        doCheck = false;
        cargoExtraArgs = "-p cala-ledger";
        
        configurePhase = ''
          export CARGO_NET_GIT_FETCH_WITH_CLI=true
          export PROTOC="${pkgs.protobuf}/bin/protoc"
          export PATH="${pkgs.protobuf}/bin:${pkgs.gitMinimal}/bin:${pkgs.coreutils}/bin:$PATH"
          export SSL_CERT_FILE="${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
          export CARGO_HTTP_CAINFO="$SSL_CERT_FILE"
          export GIT_SSL_CAINFO="$SSL_CERT_FILE"
        '';
      });

      # Check and test derivations
      checkCode = craneLib.mkCargoDerivation {
        pname = "check-code";
        version = "0.1.0";
        src = rustSource;
        cargoToml = ./Cargo.toml;
        cargoLock = ./Cargo.lock;
        cargoArtifacts = cargoArtifacts;
        SQLX_OFFLINE = true;
        cargoExtraArgs = "--all-targets --all-features";

        nativeBuildInputs = with pkgs; [
          protobuf
          cacert
          cargo-audit
          cargo-deny
        ];

        configurePhase = ''
          export CARGO_NET_GIT_FETCH_WITH_CLI=true
          export PROTOC="${pkgs.protobuf}/bin/protoc"
          export PATH="${pkgs.protobuf}/bin:${pkgs.gitMinimal}/bin:${pkgs.coreutils}/bin:$PATH"
          export SSL_CERT_FILE="${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
          export CARGO_HTTP_CAINFO="$SSL_CERT_FILE"
        '';

        buildPhaseCargoCommand = "check";
        buildPhase = ''
          cargo clippy --all-targets --all-features || true
          cargo audit
          cargo deny check
        '';
        installPhase = "touch $out";
      };

      testInCi = craneLib.mkCargoDerivation {
        pname = "test-in-ci";
        version = "0.1.0";
        src = rustSource;
        cargoToml = ./Cargo.toml;
        cargoLock = ./Cargo.lock;
        cargoArtifacts = cargoArtifacts;
        SQLX_OFFLINE = true;

        nativeBuildInputs = with pkgs; [
          cacert
          cargo-nextest
          protobuf
          gitMinimal
        ];

        configurePhase = ''
          export CARGO_NET_GIT_FETCH_WITH_CLI=true
          export PROTOC="${pkgs.protobuf}/bin/protoc"
          export PATH="${pkgs.protobuf}/bin:$PATH"
          export SSL_CERT_FILE="${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
          export CARGO_HTTP_CAINFO="$SSL_CERT_FILE"
        '';

        buildPhaseCargoCommand = "nextest run";
        buildPhase = ''
          cargo nextest run --workspace --locked --verbose
        '';

        installPhase = "touch $out";
      };

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
        ]
        ++ lib.optionals pkgs.stdenv.isDarwin [
          darwin.apple_sdk.frameworks.SystemConfiguration
        ];
      devEnvVars = rec {
        OTEL_EXPORTER_OTLP_ENDPOINT = http://localhost:4317;
        PGDATABASE = "pg";
        PGUSER = "user";
        PGPASSWORD = "password";
        PGHOST = "127.0.0.1";
        DATABASE_URL = "postgres://${PGUSER}:${PGPASSWORD}@${PGHOST}:5432/pg";
        PG_CON = "${DATABASE_URL}";
      };
    in
      with pkgs; {
        packages = {
          default = cala;
          cala = cala;
          cala-server = cala-server;
          cala-ledger = cala-ledger;
          check-code = checkCode;
          test-in-ci = testInCi;
        };
        
        checks = {
          inherit cala cala-server cala-ledger;
          
          cala-clippy = craneLib.cargoClippy (commonArgs // {
            inherit cargoArtifacts;
            cargoClippyExtraArgs = "--all-targets --all-features -- --deny warnings";
          });
          
          # Format check using filtered source
          cala-fmt = craneLib.cargoFmt {
            src = rustSourceFormatCheck;
          };
          
          cala-test = craneLib.cargoNextest (commonArgs // {
            inherit cargoArtifacts;
            partitions = 1;
            partitionType = "count";
          });
        };
        
        apps = {
          default = flake-utils.lib.mkApp {
            drv = cala-server;
          };
          
          cala-server = flake-utils.lib.mkApp {
            drv = cala-server;
          };
          
          cala-ledger = flake-utils.lib.mkApp {
            drv = cala-ledger;
          };
        };

        devShells.default = mkShell (devEnvVars
          // {
            inherit nativeBuildInputs;
          });

        formatter = alejandra;
      });
}
