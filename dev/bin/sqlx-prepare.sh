#!/usr/bin/env bash
# Build the scratch database that cala-ledger's compile-time SQLx queries are
# checked against, regenerate the `.sqlx` cache, and drop it again.
#
# The scratch database is assembled from three sources, matching what the
# workspace's migrators build at runtime:
#   - cala-ledger's own migrations, applied as-is,
#   - obix's prefix-parameterized setup template rendered with `cala_`, matching
#     CalaMailboxTables,
#   - job's migrations, resolved from its manifest rather than copied in.
# Only obix's and job's directories are resolved from their manifests: the
# scratch database always matches the versions the workspace compiles against.
# Job's schema is not optional: `cargo sqlx prepare` compiles dependencies
# online, so job's own `query!` macros expand against this database too.
#
# All SQL is plain DDL with no extension requirements, so applying the files
# file-by-file reproduces exactly what the migrators would build.
#
# Usage:
#   dev/bin/sqlx-prepare.sh [cargo-args...]
#
# Arguments are forwarded to `cargo sqlx prepare` after `-- --all-features`.
# The URL leaves this script only as an exported environment variable of the
# `cargo` child — never in argv, never printed. Needs the dev Postgres running
# (`make start-deps`) but NOT a migrated dev database: only the server is used.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

scratch_db="cala_sqlx_prepare"

if [ -z "${DATABASE_URL:-}" ]; then
  echo "error: DATABASE_URL is not set — run inside 'nix develop'" >&2
  exit 1
fi

# Split the dev URL into authority and query string so only the database path
# segment changes: the scratch database lives on the same server, with the same
# credentials and the same sslmode.
base="${DATABASE_URL%%\?*}"
query="${DATABASE_URL#"$base"}"
if [[ "$base" != *://*/?* ]]; then
  echo "error: DATABASE_URL must name a database (scheme://host:port/db)" >&2
  echo "       the dev shell's DATABASE_URL already does; do not point this elsewhere" >&2
  exit 1
fi
# sqlx applies a `?dbname=` override after the URL path, so a URL carrying one
# would silently connect somewhere other than the database this script builds.
if [[ "$query" == *dbname=* ]]; then
  echo "error: DATABASE_URL carries a dbname= override; refusing to guess which" >&2
  echo "       database this script would create and migrate" >&2
  exit 1
fi
server_url="${base%/*}/postgres${query}"
scratch_url="${base%/*}/$scratch_db${query}"

drop_database() {
  # Best effort: a failed drop only means the next run recreates the database.
  psql "$server_url" -v ON_ERROR_STOP=1 -q \
    -c "DROP DATABASE IF EXISTS $scratch_db WITH (FORCE)" \
    || echo "warning: could not drop $scratch_db; the next prepare recreates it" >&2
}

# A failed prepare must not strand the scratch database. A SIGKILLed run is
# still covered: the next run drops it before creating it.
trap drop_database EXIT

obix_templates_dir="$(
  cargo metadata --format-version 1 \
    | python3 -c '
import json
import os
import sys

packages = json.load(sys.stdin)["packages"]
package = next((p for p in packages if p["name"] == "obix"), None)
if package is None:
    sys.exit("obix is not in the dependency graph")
print(os.path.join(os.path.dirname(package["manifest_path"]), "migrations_templates"))
'
)" || {
  echo "error: could not resolve obix's migrations_templates directory" >&2
  exit 1
}

job_migrations_dir="$(
  cargo metadata --format-version 1 \
    | python3 -c '
import json
import os
import sys

packages = json.load(sys.stdin)["packages"]
package = next((p for p in packages if p["name"] == "job"), None)
if package is None:
    sys.exit("job is not in the dependency graph")
print(os.path.join(os.path.dirname(package["manifest_path"]), "migrations"))
'
)" || {
  echo "error: could not resolve job's migrations directory" >&2
  exit 1
}

shopt -s nullglob
cala_migrations=(cala-ledger/migrations/*.sql)
obix_templates=("$obix_templates_dir"/*.sql)
job_migrations=("$job_migrations_dir"/*.sql)
shopt -u nullglob

if [ ${#cala_migrations[@]} -eq 0 ]; then
  echo "error: no migrations found under cala-ledger/migrations/" >&2
  exit 1
fi
if [ ${#obix_templates[@]} -eq 0 ]; then
  echo "error: no migration templates found under $obix_templates_dir" >&2
  exit 1
fi
if [ ${#job_migrations[@]} -eq 0 ]; then
  echo "error: no migrations found under $job_migrations_dir" >&2
  exit 1
fi

# The guards above ensure all arrays are non-empty; sorting here cannot turn
# them into a bogus one-element `[""]` array when a glob matched no files.
mapfile -t cala_migrations < <(printf '%s\n' "${cala_migrations[@]}" | LC_ALL=C sort)
mapfile -t obix_templates < <(printf '%s\n' "${obix_templates[@]}" | LC_ALL=C sort)
mapfile -t job_migrations < <(printf '%s\n' "${job_migrations[@]}" | LC_ALL=C sort)

drop_database
psql "$server_url" -v ON_ERROR_STOP=1 -q -c "CREATE DATABASE $scratch_db"

for migration in "${cala_migrations[@]}"; do
  echo "applying $migration -> $scratch_db" >&2
  psql "$scratch_url" -v ON_ERROR_STOP=1 -q -f "$migration"
done

for template in "${obix_templates[@]}"; do
  echo "applying $template -> $scratch_db" >&2
  python3 -c '
import sys

with open(sys.argv[1], encoding="utf-8") as template:
    sys.stdout.write(template.read().replace("{{prefix}}", "cala_"))
' "$template" | psql "$scratch_url" -v ON_ERROR_STOP=1 -q
done

for migration in "${job_migrations[@]}"; do
  echo "applying $migration -> $scratch_db" >&2
  psql "$scratch_url" -v ON_ERROR_STOP=1 -q -f "$migration"
done

export DATABASE_URL="$scratch_url"

(cd cala-ledger && cargo sqlx prepare -- --all-features "$@")
