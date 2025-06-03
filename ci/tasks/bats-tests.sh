#!/usr/bin/env bash
set -euo pipefail

echo "--- Setting up Nix environment ---"
cachix use cala-ci
pushd repo

nix -L develop --command bash -c "
set -euo pipefail

# --- Source Helpers Early ---
# Get REPO_ROOT early to source helpers
export REPO_ROOT=\$(git rev-parse --show-toplevel)
if [[ -f \"\${REPO_ROOT}/bats/helpers.bash\" ]]; then
  echo \"--- Sourcing helpers ---\"
  source \"\${REPO_ROOT}/bats/helpers.bash\"
else
  echo \"Error: helpers.bash not found at \${REPO_ROOT}/bats/helpers.bash\"
  exit 1
fi

echo \"--- Testing Podman basic functionality ---\"
podman info || echo \"Warning: podman info failed.\"
echo \"--- Podman info done ---\"

mkdir -p /etc/containers
echo '{ \"default\": [{\"type\": \"insecureAcceptAnything\"}]}' > /etc/containers/policy.json
echo 'unqualified-search-registries = [\"docker.io\"]' > /etc/containers/registries.conf
echo \"127.0.0.1 host.containers.internal\" >> /etc/hosts

echo \"--- Starting Podman service ---\"
export DOCKER_HOST=unix:///run/podman/podman.sock
podman system service --time=0 & # Start service in background
podman_service_pid=\$! # Capture PID (optional, mainly for clarity)
echo \"--- Podman service background PID: \$podman_service_pid ---\"
sleep 5 # Wait a bit for the socket to become active
echo \"--- Podman service started (attempted) ---\"

# --- Start Dependencies ---
echo \"--- Starting Dependencies with Podman Compose ---\"
ENGINE_DEFAULT=podman bin/docker-compose-up.sh integration-deps
echo \"--- Podman-compose up done ---\"

# --- DB Setup ---
make setup-db

# --- Build Test Artifacts ---
echo \"--- Building test artifacts---\"
make build

# --- Run Bats Tests ---
echo \"--- Running BATS tests ---\"
bats -t bats
BATS_EXIT_CODE=\$?
echo \"[DEBUG] BATS command finished at \$(date) with exit code \$BATS_EXIT_CODE\"

# --- Cleanup Podman Compose Dependencies ---
echo \"--- Cleaning up Podman Compose dependencies ---\"
ENGINE_DEFAULT=podman bin/clean-deps.sh
echo \"--- Podman Compose Cleanup done ---\"

# --- Stop Podman Service ---
if ps -p \$podman_service_pid > /dev/null; then
   echo \"--- Stopping background Podman service (PID: \$podman_service_pid) ---\"
   kill \$podman_service_pid || echo \"Failed to kill podman service PID \$podman_service_pid\"
else
   echo \"--- Background Podman service (PID: \$podman_service_pid) already stopped ---\"
fi

echo \"--- All steps completed ---\"
exit \$BATS_EXIT_CODE # Exit with the Bats status code
"
