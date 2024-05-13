#!/usr/bin/env bats

load "helpers"

setup_file() {
  reset_pg
  start_server
}

teardown_file() {
  stop_server
  stop_rust_example
}


@test "rust: entities sync to server" {
  variables=$(
    jq -n \
    '{
      input: {
        name: "rust-example",
        endpoint: "http://localhost:2253"
      }
    }'
  )

  exec_graphql 'cala-outbox-import-job-create' "$variables"

  name=$(graphql_output '.data.calaOutboxImportJobCreate.job.name')
  [[ "$name" == "rust-example" ]] || exit 1;

  background cargo run --bin cala-ledger-example-rust > .rust-example-logs

  sleep 5

  exec_graphql 'list-accounts'

  name=$(graphql_output '.data.accounts.nodes[0].name')
  [[ "$name" == "MY ACCOUNT" ]] || exit 1;
}
