#!/usr/bin/env bats

load "helpers"

setup_file() {
  reset_pg
  start_server
}

teardown_file() {
  stop_server
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

  exec_graphql 'import-job-create' "$variables"

  name=$(graphql_output '.data.importJobCreate.importJob.name')
  [[ "$name" == "rust-example" ]] || exit 1;


  cargo run --bin cala-ledger-example-rust
}
