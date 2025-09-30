next-watch:
	cargo watch -s 'cargo nextest run'

clean-deps:
	./dev/bin/clean-deps.sh

start-deps:
	./dev/bin/docker-compose-up.sh integration-deps

setup-db:
	cd cala-ledger && cargo sqlx migrate run

reset-deps: clean-deps start-deps setup-db
reset-deps-perf: clean-deps start-deps setup-db
	psql postgres://user:password@localhost:5432/pg -f ./cala-perf/pg-tools/setup.sql


run-server:
	cargo run --bin cala-server -- --config ./bats/cala.yml

rust-example:
	cargo run --bin cala-ledger-example-rust

update-lib-in-nodejs-example:
	cd cala-nodejs && yarn && SQLX_OFFLINE=true yarn build
	cd examples/nodejs && rm -rf ./node_modules && yarn install

re-run-nodejs-example: clean-deps start-deps
	sleep 2
	cd examples/nodejs && yarn run start

check-code: sdl
	git diff --exit-code cala-server/schema.graphql
	SQLX_OFFLINE=true cargo fmt --check --all
	SQLX_OFFLINE=true cargo check
	SQLX_OFFLINE=true cargo clippy --package cala-server --features=
	SQLX_OFFLINE=true cargo clippy --package cala-ledger --features="import,graphql"
	SQLX_OFFLINE=true cargo clippy --package cala-ledger-core-types --features="graphql"
	SQLX_OFFLINE=true cargo clippy --workspace --exclude cala-server --exclude cala-ledger --exclude cala-ledger-core-types
	SQLX_OFFLINE=true cargo audit
	SQLX_OFFLINE=true cargo deny check

build:
	SQLX_OFFLINE=true cargo build --locked

build-nodejs-bindings:
	cd cala-nodejs && yarn && SQLX_OFFLINE=true yarn build
	cd examples/nodejs && rm -rf ./node_modules && yarn install

e2e: clean-deps start-deps build build-nodejs-bindings
	bats -t bats

sdl:
	SQLX_OFFLINE=true cargo run --bin write_sdl > cala-server/schema.graphql

sqlx-prepare:
	cd cala-ledger && cargo sqlx prepare -- --all-features
	cd cala-server && cargo sqlx prepare -- --all-features

test-in-ci:
	SQLX_OFFLINE=true cargo nextest run --verbose --locked
	SQLX_OFFLINE=true cargo test --doc
	SQLX_OFFLINE=true cargo doc --no-deps

build-x86_64-unknown-linux-musl-release:
	SQLX_OFFLINE=true cargo build --release --locked --bin cala-server --target x86_64-unknown-linux-musl

build-x86_64-apple-darwin-release:
	bin/osxcross-compile.sh
