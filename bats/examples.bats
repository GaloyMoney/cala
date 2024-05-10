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
  exec_graphql 'list-accounts'
  accounts_before=$(graphql_output '.data.accounts.nodes | length')

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
  error_msg=$(graphql_output '.errors[0].message')
  [[ "$name" == "rust-example" || "$error_msg" =~ duplicate.*jobs_name_key ]] || exit 1;

  background cargo run --bin cala-ledger-example-rust > .rust-example-logs

  sleep 5

  exec_graphql 'list-accounts'
  accounts_after=$(graphql_output '.data.accounts.nodes | length')
  [[ "$accounts_after" -gt "$accounts_before" ]] || exit 1
}
