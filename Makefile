next-watch:
	cargo watch -s 'cargo nextest run'

clean-deps:
	docker compose down

start-deps:
	docker compose up -d integration-deps

setup-db:
	cd cala-ledger && cargo sqlx migrate run

reset-deps: clean-deps start-deps setup-db
