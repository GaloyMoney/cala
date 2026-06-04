NIX_DEPS_DIR := .nix-deps

.PHONY: next-watch start-deps clean-deps setup-db reset-deps reset-deps-perf rust-example check-code build sqlx-prepare test-in-ci event-schemas check-event-schemas

next-watch:
	cargo watch -s 'cargo nextest run'

start-deps:
	@mkdir -p $(NIX_DEPS_DIR)
	@eval "$$(nix run .#dev-env)"; \
	  nix run .#nix-deps-base -- up -D; \
	  for i in $$(seq 1 60); do \
	    if nix run .#nix-deps-base -- project is-ready 2>/dev/null; then break; fi; \
	    if [ "$$i" = "60" ]; then \
	      echo "ERROR: deps not ready after 5 minutes" >&2; \
	      nix run .#nix-deps-base -- process list || true; \
	      exit 1; \
	    fi; \
	    sleep 5; \
	  done; \
	  nix run .#setup-db-dev

clean-deps:
	-@eval "$$(nix run .#dev-env)"; nix run .#nix-deps-base -- down
	chmod -R u+w $(NIX_DEPS_DIR) 2>/dev/null || true
	rm -rf $(NIX_DEPS_DIR)

setup-db:
	nix run .#setup-db-dev

reset-deps: clean-deps start-deps

reset-deps-perf: clean-deps start-deps
	@eval "$$(nix run .#dev-env)"; psql "$$DATABASE_URL" -f ./cala-perf/pg-tools/setup.sql

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
