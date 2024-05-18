#!/bin/bash

set -e

pushd repo

cat <<EOF | cargo login
${CRATES_API_TOKEN}
EOF

cargo publish -p cala-tracing --all-features --no-verify
cargo publish -p cala-ledger --all-features --no-verify
cargo publish -p cala-server --all-features --no-verify
