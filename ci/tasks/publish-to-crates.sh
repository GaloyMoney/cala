#!/bin/bash

set -e

pushd repo

cat <<EOF | cargo login
${CRATES_API_TOKEN}
EOF

cargo publish -p cala-tracing --all-features --no-verify
cargo publish -p sim-time --all-features --no-verify
cargo publish -p es-entity-macros --all-features --no-verify
cargo publish -p es-entity --all-features --no-verify
cargo publish -p cala-cel-parser --all-features --no-verify
cargo publish -p cala-cel-interpreter --all-features --no-verify
cargo publish -p cala-ledger-core-types --all-features --no-verify
cargo publish -p cala-ledger-outbox-client --all-features --no-verify
cargo publish -p cala-ledger --all-features --no-verify
cargo publish -p cala-server --all-features --no-verify
