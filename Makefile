next-watch:
	cargo watch -s 'cargo nextest run'

clean-deps:
	./dev/bin/clean-deps.sh

start-deps:
	./dev/bin/docker-compose-up.sh integration-deps

setup-db:
	@echo "Waiting for PostgreSQL and running migrations..."
	@for i in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30; do \
		cd cala-ledger && cargo sqlx migrate run 2>/dev/null && echo "Migrations complete" && exit 0; \
		echo "Attempt $$i: Database not ready, waiting..."; \
		sleep 1; \
	done; \
	echo "Database failed to become ready after 30 attempts"; \
	cd cala-ledger && cargo sqlx migrate run

reset-deps: clean-deps start-deps setup-db
reset-deps-perf: clean-deps start-deps setup-db
	psql postgres://user:password@localhost:5432/pg -f ./cala-perf/pg-tools/setup.sql


run-server:
	cargo run --bin cala-server -- --config ./bats/cala.yml

rust-example:
	cargo run --bin cala-ledger-example-rust

check-code: sdl check-event-schemas
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

e2e: clean-deps start-deps build
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

event-schemas:
	SQLX_OFFLINE=true cargo run -p cala-ledger --bin event-schemas --features json-schema

check-event-schemas: event-schemas
	git diff --exit-code cala-ledger/schemas
	@# Fail if generator produced untracked files (e.g., when a schema file was missing)
	@test -z "$$(git ls-files --others --exclude-standard -- cala-ledger/schemas)"
