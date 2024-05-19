#!/bin/bash

VERSION="$(cat version/version)-dev"

pushd repo

for file in $(find . -mindepth 2 -name Cargo.toml); do
    sed -i'' "s/^version.*/version = \"${VERSION}\"/" ${file}
done

sed -i'' "s/cel-parser\", version = .*/cel-parser\", version = \"${VERSION}\" }/" cala-cel-interpreter/Cargo.toml
sed -i'' "s/cel-interpreter\", version = .*/cel-interpreter\", version = \"${VERSION}\" }/" cala-ledger-core-types/Cargo.toml
sed -i'' "s/core-types\", version = .*/core-types\", version = \"${VERSION}\" }/" cala-ledger-outbox-client/Cargo.toml
sed -i'' "s/cel-interpreter\", version = .*/cel-interpreter\", version = \"${VERSION}\" }/" cala-ledger-outbox-client/Cargo.toml
sed -i'' "s/core-types\", version = .*/core-types\", version = \"${VERSION}\" }/" cala-ledger/Cargo.toml
sed -i'' "s/cel-interpreter\", version = .*/cel-interpreter\", version = \"${VERSION}\" }/" cala-ledger/Cargo.toml
sed -i'' "s/cala-tracing\", version = .*/cala-tracing\", version = \"${VERSION}\" }/" cala-ledger/Cargo.toml
sed -i'' "s/core-types\", version = .*/core-types\", version = \"${VERSION}\" }/" cala-server/Cargo.toml
sed -i'' "s/cala-ledger\", version = .*/cala-ledger\", version = \"${VERSION}\" }/" cala-server/Cargo.toml
sed -i'' "s/cel-interpreter\", version = .*/cel-interpreter\", version = \"${VERSION}\" }/" cala-server/Cargo.toml
sed -i'' "s/cala-ledger-outbox-client\", version = .*/cala-ledger-outbox-client\", version = \"${VERSION}\" }/" cala-server/Cargo.toml
sed -i'' "s/cala-tracing\", version = .*/cala-tracing\", version = \"${VERSION}\" }/" cala-server/Cargo.toml

cargo update --workspace

if [[ -z $(git config --global user.email) ]]; then
  git config --global user.email "bot@galoy.io"
fi
if [[ -z $(git config --global user.name) ]]; then
  git config --global user.name "CI Bot"
fi

git status
git add -A

if [[ "$(git status -s -uno)" != ""  ]]; then
  git commit -m "ci(dev): set version to ${VERSION}"
fi
