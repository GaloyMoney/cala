#!/usr/bin/env bash
set -euo pipefail

# ── Pick container engine ───────────────────────────────────────────────────────
if [[ -n "${ENGINE_DEFAULT:-}" ]]; then
  ENGINE="$ENGINE_DEFAULT"
else
  ENGINE=docker
fi

if ! command -v "$ENGINE" >/dev/null 2>&1; then
  printf 'Error: requested engine "%s" not found in $PATH\n' "$ENGINE" >&2
  exit 1
fi

# ── Down ────────────────────────────────────────────────────────────────────────
exec "$ENGINE" compose -f docker-compose.yml down -v -t 2
