#!/usr/bin/env bash
set -euo pipefail

BASE=docker-compose.yml
OVERRIDE=docker-compose.docker.yml   # contains the extra_hosts entry

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

# ── Compose file set ────────────────────────────────────────────────────────────
FILES=(-f "$BASE")
[[ "$ENGINE" == docker ]] && FILES+=(-f "$OVERRIDE")   # extra_hosts only on Docker

# ── Pull images first (prevents concurrent map writes) ─────────────────────────
echo "Pulling Docker images..."
"$ENGINE" compose "${FILES[@]}" pull --no-parallel

# ── Up ──────────────────────────────────────────────────────────────────────────
echo "Starting services..."
"$ENGINE" compose "${FILES[@]}" up -d "$@"

while ! pg_isready -d pg -p 5433 -U user; do
  echo "PostgreSQL not yet ready..."
  sleep 1
done
echo "PostgreSQL ready"
