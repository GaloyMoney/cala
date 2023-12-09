next-watch:
	cargo watch -s 'cargo nextest run'

clean-deps:
	docker compose down

start-deps:
	docker compose up -d integration-deps

setup-db:
	cd cala-ledger && cargo sqlx migrate run

reset-deps: clean-deps start-deps setup-db

rust-example:
	cargo run --bin cala-ledger-example-rust

update-nodejs-example:
	cd cala-nodejs && yarn build
	cd examples/nodejs && rm -rf ./node_modules && yarn install
