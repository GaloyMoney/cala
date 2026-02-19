#!/bin/bash

VERSION="$(cat version/version)-dev"

pushd repo

for file in $(find . -mindepth 2 -name Cargo.toml); do
    sed -i'' "s/^version.*/version = \"${VERSION}\"/" ${file}
done

sed -i'' "s/cel-parser\", version = .*/cel-parser\", version = \"${VERSION}\" }/" ./Cargo.toml
sed -i'' "s/cel-interpreter\", version = .*/cel-interpreter\", version = \"${VERSION}\" }/" ./Cargo.toml
sed -i'' "s/core-types\", version = .*/core-types\", version = \"${VERSION}\" }/" ./Cargo.toml
sed -i'' "s/cala-tracing\", version = .*/cala-tracing\", version = \"${VERSION}\" }/" ./Cargo.toml
sed -i'' "s/cala-ledger\", version = .*/cala-ledger\", version = \"${VERSION}\" }/" ./Cargo.toml
sed -i'' "s/cala-server\", version = .*/cala-server\", version = \"${VERSION}\" }/" ./Cargo.toml

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
