NIX_DEPS_DIR := .nix-deps

.PHONY: start-deps clean-deps setup-db reset-deps reset-deps-perf sqlx-prepare check-code build test-in-ci event-schemas check-event-schemas next-watch rust-example

next-watch:
	cargo watch -s 'cargo nextest run'

start-deps:
	@mkdir -p $(NIX_DEPS_DIR)
	nix run .#nix-deps-base -- up -D
	nix run .#nix-deps-base -- project is-ready --wait

clean-deps:
	-nix run .#nix-deps-base -- down
	chmod -R u+w $(NIX_DEPS_DIR) 2>/dev/null || true
	rm -rf $(NIX_DEPS_DIR)

setup-db:
	nix run .#setup-db-dev

reset-deps: clean-deps start-deps

reset-deps-perf: clean-deps start-deps
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
