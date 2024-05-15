#!/bin/bash

VERSION="$(cat version/version)-dev"

pushd repo

for file in $(find . -mindepth 2 -name Cargo.toml); do
    sed -i'' "s/^version.*/version = \"${VERSION}\"/" ${file}
done

sed -i'' "s/cala-cel-parser\", version = .*/cala-cel-parser\", version = \"${VERSION}\" }/" cala-cel-parser/Cargo.toml
sed -i'' "s/cala-cel-interpreter\", version = .*/cala-cel-interpreter\", version = \"${VERSION}\" }/" cala-cel-interpreter/Cargo.toml
sed -i'' "s/cala-ledger-core-types\", version = .*/cala-ledger-core-types\", version = \"${VERSION}\" }/" cala-ledger-core-types/Cargo.toml
sed -i'' "s/cala-ledger-outbox-client\", version = .*/cala-ledger-outbox-client\", version = \"${VERSION}\" }/" cala-ledger-outbox-client/Cargo.toml
sed -i'' "s/cala-ledger\", version = .*/cala-ledger\", version = \"${VERSION}\" }/" cala-ledger/Cargo.toml
sed -i'' "s/cala-server\", version = .*/cala-server\", version = \"${VERSION}\" }/" cala-server/Cargo.toml

if [[ -z $(git config --global user.email) ]]; then
  git config --global user.email "bot@cepler.dev"
fi
if [[ -z $(git config --global user.name) ]]; then
  git config --global user.name "CI Bot"
fi

git status
git add -A

if [[ "$(git status -s -uno)" != ""  ]]; then
  git commit -m "ci(dev): set version to ${VERSION}"
fi
