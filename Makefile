NIX_DEPS_DIR := .nix-deps

# Seconds each fuzz target runs in `make fuzz`.
FUZZ_TIME := 60

.PHONY: next-watch start-deps clean-deps setup-db reset-deps reset-deps-perf rust-example check-code build sqlx-prepare event-schemas check-event-schemas fuzz

next-watch:
	cargo watch -s 'cargo nextest run'

start-deps:
	@mkdir -p $(NIX_DEPS_DIR)
	@set -e; \
	  eval "$$(nix run .#dev-env)"; \
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
	SQLX_OFFLINE=true cargo doc --no-deps

event-schemas:
	SQLX_OFFLINE=true cargo run -p cala-ledger --bin event-schemas --features json-schema

check-event-schemas: event-schemas
	git diff --exit-code cala-ledger/schemas
	@# Fail if generator produced untracked files (e.g., when a schema file was missing)
	@test -z "$$(git ls-files --others --exclude-standard -- cala-ledger/schemas)"

# Coverage-guided fuzzing via the shared script (ci/vendor/tasks/fuzz.sh), also
# used by `nix run .#fuzz` and the Concourse `fuzz` job. Runs all targets in
# parallel for $(FUZZ_TIME)s; the corpus lives in fuzz/corpus/ (gitignored).
fuzz:
	SQLX_OFFLINE=true FUZZ_SECONDS=$(FUZZ_TIME) bash ci/vendor/tasks/fuzz.sh

# One-time bootstrap of the GCS fuzz corpus from the local fuzz/seeds/ set
# (gitignored — the corpus, including seeds, lives in GCS, not git; matches
# es-entity). Copies seeds into fuzz/corpus/, and when GCS_BUCKET is set
# (and gcloud is authed) uploads a corpus-v<ts>.tgz to the same prefix the
# Concourse `fuzz` job reads/writes. Without GCS_BUCKET it just builds the
# local corpus and prints the upload command.
fuzz-seed-corpus:
	@set -e; \
	for t in $$(cd fuzz && cargo fuzz list); do \
	  mkdir -p fuzz/corpus/$$t; \
	  [ -d fuzz/seeds/$$t ] && cp -R fuzz/seeds/$$t/. fuzz/corpus/$$t/ 2>/dev/null || true; \
	done; \
	if [ -z "$$GCS_BUCKET" ]; then \
	  echo "GCS_BUCKET not set — seeded local fuzz/corpus/ from fuzz/seeds/."; \
	  echo "To bootstrap the CI corpus (needs gcloud auth):"; \
	  echo "  GCS_BUCKET=<bucket> make fuzz-seed-corpus"; \
	else \
	  ts="corpus-v$$(date -u +%Y%m%d-%H%M%S).tgz"; \
	  tar -czf "$$ts" -C fuzz corpus; \
	  gsutil cp "$$ts" "gs://$$GCS_BUCKET/cala-artifacts/fuzz-corpus/"; \
	  rm -f "$$ts"; \
	  echo "uploaded seed corpus -> gs://$$GCS_BUCKET/cala-artifacts/fuzz-corpus/$$ts"; \
	fi
