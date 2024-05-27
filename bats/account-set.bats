#!/usr/bin/env bats

load "helpers"

setup_file() {
  start_server
}

teardown_file() {
  stop_server
}

@test "cala: create an account set" {
  journal_id=$(random_uuid)
  variables=$(
    jq -n \
    --arg journal_id "$journal_id" \
    '{
        "input": {
          "journalId": $journal_id,
          "name": "General Ledger",
        }
    }'
  )
  exec_graphql 'journal-create' "$variables"
  output=$(graphql_output '.data.journalCreate.journal.journalId')
  [[ $output ]] || exit 1
  
  # create an account set
  account_set_id=$(random_uuid)
  variables=$(
    jq -n \
    --arg account_set_id "$account_set_id" \
    --arg journal_id "$journal_id" \
    '{
      "input": {
        "accountSetId": $account_set_id,
        "journalId": $journal_id,
        "name": "Account Set",
        "normalBalanceType": "CREDIT"
      }
    }'
  )
  exec_graphql 'account-set-create' "$variables"
  output=$(graphql_output '.data.accountSetCreate.accountSet.accountSetId')
  [[ "$output" != "null" ]] || exit 1
}
