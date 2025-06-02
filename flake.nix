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

      toolchain = (with builtins; fromTOML (readFile ./rust-toolchain.toml)).toolchain;

      rustToolchain = pkgs.pkgsBuildHost.rust-bin.fromRustupToolchain {
        inherit (toolchain) channel profile targets components;
      };

      # rustToolchain = rustVersion.override {
      #   extensions = ["rust-analyzer" "rust-src"];
      # };

      rustToolchainCi = pkgs.pkgsBuildHost.rust-bin.fromRustupToolchain {
        inherit (toolchain) channel components;
        profile = "minimal";
        # targets = ["x86_64-unknown-linux-musl"];
      };

      # Common dependencies used in both dev and CI
      commonDeps = with pkgs; [
        postgresql
        sqlx-cli
        cargo-nextest
        jq
        bats
      ];

      # Development-specific dependencies
      devDeps = with pkgs; [
        rustToolchain
        cachix

        alejandra
        podman
        podman-compose
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
        bashInteractive
        gnumake
        gcc
        rustToolchainCi
        coreutils
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

      ciShell = pkgs.mkShell (
        devEnvVars
        // {
          buildInputs = commonDeps ++ ciDeps;
        }
      );

      ciNixImage = pkgs.dockerTools.buildImageWithNixDb {
        name = "cala-ci-nix";

        contents = commonDeps ++ ciDeps;
      };

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

        config = {
          Env = [
            # required because /var/tmp does not exist in the image
            "TMPDIR=/tmp"
          ] ++ (pkgs.lib.mapAttrsToList (k: v: "${k}=${v}") devEnvVars);
        };
      };
    in
      with pkgs; {
        devShells.default = devShell;
        devShells.ci = ciShell;

        packages.ciImage = ciImage;
        packages.ciNixImage = ciNixImage;

        formatter = alejandra;
      });
}
