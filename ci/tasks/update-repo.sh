#!/bin/bash

set -eu

# ----------- UPDATE REPO -----------
git config --global user.email "bot@galoy.io"
git config --global user.name "CI Bot"

pushd repo

VERSION="$(cat ../version/version)"

cat <<EOF >new_change_log.md
# [cala release v${VERSION}](https://github.com/GaloyMoney/cala/releases/tag/${VERSION})

$(cat ../artifacts/gh-release-notes.md)

$(cat CHANGELOG.md)
EOF
mv new_change_log.md CHANGELOG.md

for file in $(find . -mindepth 2 -name Cargo.toml); do
  sed -i'' "s/^version.*/version = \"${VERSION}\"/" ${file}
done

sed -i'' "s/cel-parser\", version = .*/cel-parser\", version = \"${VERSION}\" }/" cala-cel-interpreter/Cargo.toml
sed -i'' "s/cel-interpreter\", version = .*/cel-interpreter\", version = \"${VERSION}\" }/" cala-ledger-core-types/Cargo.toml
sed -i'' "s/core-types\", version = .*/core-types\", version = \"${VERSION}\" }/" cala-ledger-outbox-client/Cargo.toml
sed -i'' "s/cel-interpreter\", version = .*/cel-interpreter\", version = \"${VERSION}\" }/" cala-ledger-outbox-client/Cargo.toml

sed -i'' "s/sim-time\", version = .*/sim-time\", version = \"${VERSION}\" }/" lib/es-entity/Cargo.toml
sed -i'' "s/es-entity-macros\", version = .*/es-entity-macros\", version = \"${VERSION}\" }/" lib/es-entity/Cargo.toml

sed -i'' "s/core-types\", version = .*/core-types\", version = \"${VERSION}\" }/" cala-ledger/Cargo.toml
sed -i'' "s/cel-interpreter\", version = .*/cel-interpreter\", version = \"${VERSION}\" }/" cala-ledger/Cargo.toml
sed -i'' "s/cala-tracing\", version = .*/cala-tracing\", version = \"${VERSION}\" }/" cala-ledger/Cargo.toml
sed -i'' "s/es-entity\", version = .*/es-entity\", version = \"${VERSION}\" }/" cala-ledger/Cargo.toml

sed -i'' "s/core-types\", version = .*/core-types\", version = \"${VERSION}\" }/" cala-server/Cargo.toml
sed -i'' "s/cala-ledger\", version = .*/cala-ledger\", version = \"${VERSION}\" }/" cala-server/Cargo.toml
sed -i'' "s/cel-interpreter\", version = .*/cel-interpreter\", version = \"${VERSION}\" }/" cala-server/Cargo.toml
sed -i'' "s/cala-ledger-outbox-client\", version = .*/cala-ledger-outbox-client\", version = \"${VERSION}\" }/" cala-server/Cargo.toml
sed -i'' "s/cala-tracing\", version = .*/cala-tracing\", version = \"${VERSION}\" }/" cala-server/Cargo.toml

cargo update --workspace

git status
git add .

if [[ "$(git status -s -uno)" != ""  ]]; then
  git commit -m "ci(release): release version $(cat ../version/version)"
fi
