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
    in
      with pkgs; {
        devShells.default = mkShell (devEnvVars
          // {
            inherit nativeBuildInputs;
          });

        formatter = alejandra;
      });
}
