next-watch:
	cargo watch -s 'cargo nextest run'

clean-deps:
	docker compose down

start-deps:
	docker compose up -d integration-deps

wait-for-dbs:
	@echo "Waiting for databases to be ready..."
	@until docker compose exec -T server-pg pg_isready -U user; do sleep 1; done
	@until docker compose exec -T examples-pg pg_isready -U user; do sleep 1; done
	@echo "Databases are ready"

setup-db: wait-for-dbs
	cd cala-ledger && cargo sqlx migrate run
	cd cala-server && cargo sqlx migrate run --ignore-missing

reset-deps: clean-deps start-deps setup-db

run-server:
	cargo run --bin cala-server -- --config ./bats/cala.yml

rust-example:
	cargo run --bin cala-ledger-example-rust

update-lib-in-nodejs-example:
	cd cala-nodejs && SQLX_OFFLINE=true yarn build
	cd examples/nodejs && rm -rf ./node_modules && yarn install

re-run-nodejs-example: clean-deps start-deps
	sleep 2
	cd examples/nodejs && yarn run start

check-code: sdl
	git diff --exit-code cala-server/schema.graphql
	SQLX_OFFLINE=true cargo fmt --check --all
	SQLX_OFFLINE=true cargo check
	SQLX_OFFLINE=true cargo clippy --package es-entity --all-features
	SQLX_OFFLINE=true cargo clippy --package cala-server --features=
	SQLX_OFFLINE=true cargo clippy --package cala-ledger --features="import,graphql"
	SQLX_OFFLINE=true cargo clippy --package cala-ledger-core-types --features="graphql"
	SQLX_OFFLINE=true cargo clippy --workspace --exclude es-entity --exclude cala-server --exclude cala-ledger --exclude cala-ledger-core-types
	SQLX_OFFLINE=true cargo audit
	SQLX_OFFLINE=true cargo deny check

build:
	SQLX_OFFLINE=true cargo build --locked

e2e: clean-deps start-deps build
	bats -t bats

sdl:
	SQLX_OFFLINE=true cargo run --bin write_sdl > cala-server/schema.graphql

sqlx-prepare:
	cd cala-ledger && cargo sqlx prepare -- --all-features
	cd cala-server && cargo sqlx prepare -- --all-features

test-in-ci: start-deps setup-db
	cargo nextest run --verbose --locked
	cargo test --doc
	cargo doc --no-deps

build-x86_64-unknown-linux-musl-release:
	SQLX_OFFLINE=true cargo build --release --locked --bin cala-server --target x86_64-unknown-linux-musl

build-x86_64-apple-darwin-release:
	bin/osxcross-compile.sh
