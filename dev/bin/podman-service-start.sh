#!/usr/bin/env bash
set -euo pipefail

echo "--- Configuring Podman ---"

if [ "$(uname)" = "Linux" ]; then
    echo "Applying Linux-specific podman configuration..."
    mkdir -p /etc/containers
    echo '{ "default": [{"type": "insecureAcceptAnything"}]}' > /etc/containers/policy.json || true
    echo 'unqualified-search-registries = ["docker.io"]' > /etc/containers/registries.conf || true
    grep -q "host.containers.internal" /etc/hosts || echo "127.0.0.1 host.containers.internal" >> /etc/hosts || true
else
    echo "Non-Linux system detected, skipping container configuration"
fi

echo "--- Podman configuration done ---"
echo "--- Starting Podman service ---"

if [ "$(uname)" = "Linux" ]; then
    echo "Checking if podman socket is working..."

    # Try system socket first, then user socket
    SYSTEM_SOCKET="/run/podman/podman.sock"
    USER_SOCKET="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/podman/podman.sock"

    if [ -S "$SYSTEM_SOCKET" ] && CONTAINER_HOST="unix://$SYSTEM_SOCKET" timeout 3s podman version >/dev/null 2>&1; then
        echo "System podman socket already working!"
    elif [ -S "$USER_SOCKET" ] && CONTAINER_HOST="unix://$USER_SOCKET" timeout 3s podman version >/dev/null 2>&1; then
        echo "User podman socket already working!"
    else
        echo "Starting podman system service..."

        # Try to create system socket directory with sudo, fall back to user socket
        if sudo mkdir -p /run/podman 2>/dev/null; then
            echo "Using system socket at $SYSTEM_SOCKET"
            podman system service --time=0 "unix://$SYSTEM_SOCKET" &
            SOCKET_PATH="$SYSTEM_SOCKET"
        else
            echo "Cannot create system socket, using user socket at $USER_SOCKET"
            mkdir -p "$(dirname "$USER_SOCKET")"
            podman system service --time=0 "unix://$USER_SOCKET" &
            SOCKET_PATH="$USER_SOCKET"
        fi

        echo "Waiting for socket to be created..."
        for i in 1 2 3 4 5; do
            if [ -S "$SOCKET_PATH" ] && CONTAINER_HOST="unix://$SOCKET_PATH" timeout 3s podman version >/dev/null 2>&1; then
                echo "Socket created and working!"
                break
            fi
            echo "Waiting... ($i/5)"
            sleep 2
        done
    fi
fi

echo "--- Podman service ready ---"
