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
  };
  outputs = {
    self,
    nixpkgs,
    flake-utils,
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
        extensions = ["rust-analyzer" "rust-src"];
      };

      # Common dependencies used in both dev and CI
      commonDeps = with pkgs; [
        rustToolchain
        postgresql
        sqlx-cli
        cargo-nextest
        jq
        bats
        cachix
        podman
        podman-compose
      ];

      # Development-specific dependencies
      devDeps = with pkgs; [
        alejandra
        cargo-audit
        cargo-watch
        cargo-deny
        bacon
        docker-compose
        napi-rs-cli
        yarn
        nodejs
        typescript
        ytt
      ] ++ lib.optionals pkgs.stdenv.isDarwin [
        darwin.apple_sdk.frameworks.SystemConfiguration
      ];

      # CI-specific dependencies
      ciDeps = with pkgs; [
        podman
        podman-compose
        bash
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

      # Development shell with all development tools
      devShell = pkgs.mkShell (
        devEnvVars
        // {
          buildInputs = commonDeps ++ devDeps;
        }
      );

      # Build the CI image with only the necessary dependencies
      ciImage = with pkgs.dockerTools; buildLayeredImage {
        name = "cala-ci";
        tag = "latest";

        fromImage = pullImage {
          imageName = "nixpkgs/cachix-flakes";
          imageDigest = "sha256:48339bf3bc6cf7ab879f8973ba6728261044cefb873b6cc639bad63050e64538";
          sha256 = "0xmbmr34qrrrcyihfppbx81mkx2jain8gnmd8psbikw1bs691gr7";
        };

        contents = pkgs.buildEnv {
          name = "root";
          paths = commonDeps ++ ciDeps;
          pathsToLink = [ "/bin" ];
        };
      };
    in
      with pkgs; {
        devShells.default = devShell;

        packages.ciImage = ciImage;

        formatter = alejandra;
      });
}
