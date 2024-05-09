#!/usr/bin/env bats

load "helpers"

# setup_file() {
#   reset_pg
#   start_server
# }

# teardown_file() {
#   stop_server
#   stop_rust_example
# }


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


  background cargo run --bin cala-ledger-example-rust > .rust-example-logs

  sleep 5

  exec_graphql 'list-accounts'

  name=$(graphql_output '.data.accounts.nodes[0].name')
  [[ "$name" == "MY ACCOUNT" ]] || exit 1;
}

# task:
# Figure out a way to
# - run the same tests for every supported language
#   with minimal code duplication
#
# Figure out useful idempotency workflow
# - run bats test but don't restart server if its already running as a seperate process
# - keep example servers alive if I need them (for outbox syncing)
# - make it so that I can re-start the example servers and still simply verify that things are working
#
#
#
#

# file: sync-account
# test js
# test rust
# test golang

# file: rust
# - sync account
# - sync journal
#
# file: JS
# - sync account
# - sync journal
#
# AND NOW WE ADD
# - sync xxx to rust
#
# - copy paste -> JS file
#
#
