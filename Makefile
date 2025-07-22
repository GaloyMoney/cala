next-watch:
	nix develop --command cargo watch -s 'cargo nextest run'

clean-deps:
	docker compose down

start-deps:
	docker compose up -d integration-deps

setup-db:
	cd cala-ledger && nix develop --command cargo sqlx migrate run
	cd cala-server && nix develop --command cargo sqlx migrate run --ignore-missing

reset-deps: clean-deps start-deps setup-db

run-server:
	nix run .#cala-server -- --config ./bats/cala.yml

rust-example:
	nix run .#cala-ledger-example-rust

update-lib-in-nodejs-example:
	cd cala-nodejs && SQLX_OFFLINE=true nix develop --command yarn build
	cd examples/nodejs && rm -rf ./node_modules && yarn install

re-run-nodejs-example: clean-deps start-deps
	sleep 2
	cd examples/nodejs && yarn run start

check-code:
	nix run .#checkCode

build:
	nix build

e2e: clean-deps start-deps build
	nix develop --command bats -t bats

sdl:
	nix run .#write-sdl > cala-server/schema.graphql

sqlx-prepare:
	cd cala-ledger && nix develop --command cargo sqlx prepare -- --all-features
	cd cala-server && nix develop --command cargo sqlx prepare -- --all-features

test-in-ci: start-deps setup-db
	nix run .#testInCi

build-x86_64-unknown-linux-musl-release:
	nix build .#cala-server

build-x86_64-apple-darwin-release:
	nix build .#cala-server

dev:
	nix develop

run-ledger:
	nix run .#cala-ledger

run-outbox-client:
	nix run .#cala-ledger-outbox-client

run-cel-parser:
	nix run .#cala-cel-parser

