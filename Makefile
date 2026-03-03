next-watch:
	cargo watch -s 'cargo nextest run'

clean-deps:
	./dev/bin/clean-deps.sh

start-deps:
	./dev/bin/docker-compose-up.sh integration-deps

setup-db:
	@echo "Waiting for PostgreSQL and running migrations..."
	@for i in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30; do \
		(cd cala-ledger && cargo sqlx migrate run 2>/dev/null) && echo "Migrations complete" && exit 0; \
		echo "Attempt $$i: Database not ready, waiting..."; \
		sleep 1; \
	done; \
	echo "Database failed to become ready after 30 attempts"; \
	cd cala-ledger && cargo sqlx migrate run

reset-deps: clean-deps start-deps setup-db
reset-deps-perf: clean-deps start-deps setup-db
	psql postgres://user:password@localhost:5432/pg -f ./cala-perf/pg-tools/setup.sql

rust-example:
	cargo run --bin cala-ledger-example-rust

check-code:
	nix flake check

build:
	SQLX_OFFLINE=true cargo build --locked

sqlx-prepare:
	cd cala-ledger && cargo sqlx prepare -- --all-features

test-in-ci:
	SQLX_OFFLINE=true cargo nextest run --verbose --locked
	SQLX_OFFLINE=true cargo test --doc
	SQLX_OFFLINE=true cargo doc --no-deps

event-schemas:
	SQLX_OFFLINE=true cargo run -p cala-ledger --bin event-schemas --features json-schema

check-event-schemas: event-schemas
	git diff --exit-code cala-ledger/schemas
	@# Fail if generator produced untracked files (e.g., when a schema file was missing)
	@test -z "$$(git ls-files --others --exclude-standard -- cala-ledger/schemas)"
