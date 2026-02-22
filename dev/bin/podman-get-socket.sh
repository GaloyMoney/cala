#!/usr/bin/env bash
set -euo pipefail

# Determine the correct podman socket to use
# On macOS, podman often uses SSH connections to a VM, so we shouldn't set DOCKER_HOST

# Check if we're on macOS and podman is using SSH connections
if [[ "$(uname)" == "Darwin" ]]; then
    # Check if podman is using SSH connections (typical for macOS)
    if podman system connection list 2>/dev/null | grep -q "ssh://"; then
        # On macOS with SSH connections, don't set DOCKER_HOST
        # Return special value to indicate no socket should be used
        echo "NO_SOCKET"
        exit 0
    fi
fi

# For Linux or other cases, use Unix sockets
SYSTEM_SOCKET="/run/podman/podman.sock"
USER_SOCKET="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/podman/podman.sock"

if [ -S "$SYSTEM_SOCKET" ] && CONTAINER_HOST="unix://$SYSTEM_SOCKET" timeout 3s podman version >/dev/null 2>&1; then
    echo "unix://$SYSTEM_SOCKET"
elif [ -S "$USER_SOCKET" ] && CONTAINER_HOST="unix://$USER_SOCKET" timeout 3s podman version >/dev/null 2>&1; then
    echo "unix://$USER_SOCKET"
else
    # Default fallback (will likely fail, but provides a reasonable default)
    echo "unix://$SYSTEM_SOCKET"
fi
