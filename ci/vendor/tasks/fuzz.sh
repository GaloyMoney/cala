#!/usr/bin/env bash

#! Shared cargo-fuzz runner — the single source of truth for fuzzing logic,
#! vendored from galoy-concourse-shared into each repo's ci/vendor/tasks/.
#!
#! Used by:
#!   - the Concourse `fuzz` job (fuzz_job() in pipeline-fragments.lib.yml):
#!     the restore-corpus / store-corpus steps pass the corpus in/out as a
#!     tarball via CORPUS_TARBALL_IN/OUT (GCS handled by those gsutil steps)
#!   - `make fuzz` / a repo-specific flake app, for local runs
#!
#! Repo-agnostic: discovers targets via `cargo fuzz list`. The repo must ship a
#! `fuzz/` cargo-fuzz crate.
#!
#! Env vars (all optional):
#!   FUZZ_SECONDS        seconds to fuzz each target (default: 60)
#!   FUZZ_JOBS           libFuzzer `-jobs` per target; cores ~= #targets * FUZZ_JOBS
#!   CORPUS_TARBALL_IN   glob of a corpus tarball to extract before fuzzing
#!   CORPUS_TARBALL_OUT  path to write the evolved corpus tarball after fuzzing
#!
#! Requires: bash, git, cargo, tar, and cargo-fuzz (auto-installed if missing).

set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$ROOT"

if ! command -v cargo-fuzz >/dev/null 2>&1; then
  echo "cargo-fuzz not found on PATH; installing..."
  cargo install cargo-fuzz --locked
fi

FUZZ_SECONDS="${FUZZ_SECONDS:-60}"

#! Restore the corpus (no-op unless CORPUS_TARBALL_IN is set and matches a file).
mkdir -p fuzz/corpus
if [ -n "${CORPUS_TARBALL_IN:-}" ] && compgen -G "$CORPUS_TARBALL_IN" >/dev/null; then
  echo "restoring corpus from $CORPUS_TARBALL_IN"
  tar -xzf $CORPUS_TARBALL_IN -C fuzz/
fi

mapfile -t targets < <(cd fuzz && cargo fuzz list)
echo "discovered ${#targets[@]} target(s): ${targets[*]}"

(cd fuzz && cargo fuzz build --sanitizer=none)

JOBS_ARG=""
if [ -n "${FUZZ_JOBS:-}" ]; then
  JOBS_ARG="-jobs=$FUZZ_JOBS"
fi

echo "fuzzing ${#targets[@]} target(s) in parallel for ${FUZZ_SECONDS}s${JOBS_ARG:+ ($JOBS_ARG per target)}"
pids=""
rc=0
for target in "${targets[@]}"; do
  (cd fuzz && cargo fuzz run "$target" --sanitizer=none -- \
    -max_total_time="$FUZZ_SECONDS" -timeout=25 $JOBS_ARG \
    -artifact_prefix="artifacts/$target/") &
  pids="$pids $!"
done
for p in $pids; do
  wait "$p" || rc=1
done

if [ "$rc" -ne 0 ]; then
  echo "==== FUZZ CRASH DETECTED ===="
  find fuzz/artifacts -type f -print || true
  exit "$rc"
fi

#! Package the evolved corpus (no-op unless CORPUS_TARBALL_OUT is set).
if [ -n "${CORPUS_TARBALL_OUT:-}" ]; then
  mkdir -p "$(dirname "$CORPUS_TARBALL_OUT")"
  tar -czf "$CORPUS_TARBALL_OUT" -C fuzz corpus
  echo "packaged corpus -> $CORPUS_TARBALL_OUT"
fi
