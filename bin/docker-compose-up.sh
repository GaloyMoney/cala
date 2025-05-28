#!/usr/bin/env bash
set -euo pipefail

FILE=docker-compose.yml

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
echo "Pulling Docker images..."
"$ENGINE" compose -f "$FILE" pull

# ── Up ──────────────────────────────────────────────────────────────────────────
echo "Starting services..."
"$ENGINE" compose -f "$FILE" up -d "$@"

while ! pg_isready -h localhost -p 5432 -U user -d pg; do
  echo "PostgreSQL not yet ready..."
  sleep 1
done
echo "PostgreSQL ready"
