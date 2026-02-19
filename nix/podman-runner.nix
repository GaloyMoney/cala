{
  pkgs,
  lib,
  stdenv,
}: let
  # Build the podman-compose runner as a derivation
  podman-compose-runner = pkgs.stdenv.mkDerivation {
    pname = "podman-compose-runner";
    version = "0.1.0";

    # No source needed for a wrapper script
    dontUnpack = true;

    buildInputs = with pkgs; [
      makeWrapper
    ];

    installPhase = ''
      mkdir -p $out/bin

      # Create the runner script that uses podman-compose directly
      cat > $out/bin/podman-compose-runner << 'EOF'
      #!/usr/bin/env bash
      set -e

      # On macOS, check if podman machine exists and start it if needed
      if [[ "$OSTYPE" == "darwin"* ]]; then
        if podman machine list --format json | jq -e '.[] | select(.Name == "podman-machine-default")' >/dev/null 2>&1; then
          # Machine exists, check if it's running
          if ! podman machine list --format json | jq -e '.[] | select(.Name == "podman-machine-default" and .Running == true)' >/dev/null 2>&1; then
            echo "Starting podman machine..."
            podman machine start
          fi
        else
          echo "No podman machine found. Creating and starting podman-machine-default..."
          podman machine init
          podman machine start
        fi

        # Set up minimal container configs for macOS
        mkdir -p ~/.config/containers
        echo 'unqualified-search-registries = ["docker.io"]' > ~/.config/containers/registries.conf
        echo '{"default":[{"type":"insecureAcceptAnything"}]}' > ~/.config/containers/policy.json
      else
        # On Linux, setup container configs for rootless operation
        echo "Using podman on Linux..."

        # Set up runtime directory for rootless containers
        export XDG_RUNTIME_DIR="''${XDG_RUNTIME_DIR:-/tmp/podman-runtime-$(id -u)}"
        mkdir -p "$XDG_RUNTIME_DIR"

        # Create necessary temp directories
        mkdir -p /var/tmp
        mkdir -p /tmp
        export TMPDIR=/tmp

        # Set up minimal container configs
        mkdir -p ~/.config/containers
        echo 'unqualified-search-registries = ["docker.io"]' > ~/.config/containers/registries.conf
        echo '{"default":[{"type":"insecureAcceptAnything"}]}' > ~/.config/containers/policy.json

        # Don't specify network backend - let podman use its default (netavark)
        # But ensure iptables is available in PATH

        # Debug: Check podman version and info
        echo "Checking podman installation..."
        if ! podman version >/dev/null 2>&1; then
          echo "ERROR: podman version failed. Output:"
          podman version 2>&1 || true
        fi

        # Try to get podman info for debugging
        echo "Getting podman info..."
        podman info 2>&1 || true

        # Check if we're in a container environment (CI)
        if [[ -f /.dockerenv ]] || [[ -n "''${container:-}" ]]; then
          echo "Running in container environment"
        fi

        # Final check
        if podman ps >/dev/null 2>&1; then
          echo "Podman is working correctly"
        else
          echo "WARNING: podman ps test failed, but continuing anyway..."
        fi
      fi

      # Use podman-compose directly (it handles the socket connection internally)
      exec podman-compose "$@"
      EOF

      chmod +x $out/bin/podman-compose-runner

      # Wrap the script with the required dependencies
      wrapProgram $out/bin/podman-compose-runner \
        --prefix PATH : ${pkgs.lib.makeBinPath (
        [
          pkgs.podman
          pkgs.podman-compose
          pkgs.coreutils
          pkgs.bash
          pkgs.jq
        ]
        ++ pkgs.lib.optionals pkgs.stdenv.isLinux [
          pkgs.fuse-overlayfs
          pkgs.iptables
          pkgs.netavark
          pkgs.aardvark-dns
        ]
      )}
    '';

    meta = with pkgs.lib; {
      description = "Podman-compose runner that auto-manages podman machine on macOS";
      license = licenses.mit;
      platforms = platforms.all;
    };
  };
in {
  # Default package is the full runner with machine management
  podman-compose-runner = podman-compose-runner;
}
