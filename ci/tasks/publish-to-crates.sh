#!/bin/bash

set -e

publish_crate() {
  local crate=$1
  local output
  if output=$(cargo publish -p "$crate" --all-features --no-verify 2>&1); then
    echo "Published $crate"
  elif echo "$output" | grep -q "already exists"; then
    echo "Skipping $crate - version already published"
  else
    echo "$output"
    echo "Failed to publish $crate"
    return 1
  fi
}

pushd repo

cat <<EOF | cargo login
${CRATES_API_TOKEN}
EOF

publish_crate cala-tracing
publish_crate cala-cel-interpreter
publish_crate cala-ledger-core-types
publish_crate cala-ledger
