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
        inherit (toolchain) channel;
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
        curl
        cachix
      ];

      # Development-specific dependencies
      devDeps = with pkgs; [
        rustToolchain

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
        libtool
        gcc
        gnumake
        rustToolchainCi
        coreutils
        nix
        bashInteractive
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

      ciEnvVars = [
        # "ENV=/etc/profile.d/nix.sh"
        # "BASH_ENV=/etc/profile.d/nix.sh"
        # "NIX_BUILD_SHELL=/bin/bash"
        # "PAGER=cat"
        # "PATH=/usr/bin:/bin"
        "USER=nobody"
      ];

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

      ciNixImage = with pkgs; dockerTools.buildLayeredImage {
        name = "cala-ci-nix";
        tag = "latest";
        maxLayers = 120;

        contents = buildEnv {
          name = "root";
          paths = commonDeps ++ ciDeps ++ (with pkgs.dockerTools; [
            binSh
            caCertificates
            usrBinEnv
            fakeNss
          ]);
          pathsToLink = [ "/bin" "/etc" "/usr" ];
        };

        extraCommands = ''
          # make sure /tmp exists
          mkdir -m 1777 tmp
        '';

        config = {
          Env = [
            # required because /var/tmp does not exist in the image
            "TMPDIR=/tmp"
          ] ++ (lib.mapAttrsToList (k: v: "${k}=${v}") devEnvVars) ++ ciEnvVars;
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
