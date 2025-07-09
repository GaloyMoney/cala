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
        devShells.default = mkShell (devEnvVars
          // {
            inherit nativeBuildInputs;
          });

        # --- New lightweight shell for CI -----------
        devShells.ci = mkShell {
          inherit (devEnvVars) PGDATABASE PGUSER PGPASSWORD PGHOST DATABASE_URL PG_CON;
          nativeBuildInputs = with pkgs; [
            rustToolchain      # same version you already pin
            cargo-nextest
            sqlx-cli
            postgresql         # client binaries for sqlx
            podman podman-compose
            jq curl
            bashInteractive
          ];
        };

        # optional: a derivation to make pre-built caches clearer
        packages.ci-env = devShells.ci;

        formatter = alejandra;
      });
}
