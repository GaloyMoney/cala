#!/usr/bin/env bash
set -euo pipefail

# ── Pick container engine ───────────────────────────────────────────────────────
if [[ -n "${ENGINE_DEFAULT:-}" ]]; then            # honour explicit choice
  ENGINE="$ENGINE_DEFAULT"
else                                               # otherwise prefer docker
  ENGINE=docker
fi

# ensure the binary is on PATH
if ! command -v "$ENGINE" >/dev/null 2>&1; then
  printf 'Error: requested engine "%s" not found in $PATH\n' "$ENGINE" >&2
  exit 1
fi

# ── Pull images first (prevents concurrent map writes) ─────────────────────────
# Only pull in CI to avoid slow re-pulls during local development
if [[ "${CI:-false}" == "true" ]]; then
  echo "Pulling Docker images..."
  "$ENGINE" compose -f docker-compose.yml pull
fi

# ── Up ──────────────────────────────────────────────────────────────────────────
echo "Starting services..."
"$ENGINE" compose -f docker-compose.yml up -d "$@"

wait4x postgresql ${PG_CON} --timeout 120s
