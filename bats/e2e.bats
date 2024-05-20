#!/usr/bin/env bats

load "operations"
load "helpers"

setup_file() {
  start_server
}

teardown_file() {
  stop_server
  stop_rust_example
}

@test "cala: journal create" {
  variables=$(
    jq -n \
    '{
        "input": {
          "journalId": "822cb59f-ce51-4837-8391-2af3b7a5fc51",
          "name": "General Ledger",
          "description": "Primary journal for Lava."
        }
      }
    '
  )
  exec_graphql 'journal-create' "$variables"
  journal_id=$(graphql_output '.data.journalCreate.journal.journalId')
  [[ $journal_id == "822cb59f-ce51-4837-8391-2af3b7a5fc51" ]] || exit 1
}

@test "cala: account create" {
  variables=$(
    jq -n \
    '{
      "input": {
        "accountId": "1fd1dd3e-33fe-4ef5-9d58-676ef8d306b5",
        "name": "Alice - Checking",
        "code": "ALICE.CHECKING",
        "description": "Alice checking account",
        "normalBalanceType": "CREDIT"
      }
    }'
  )
  exec_graphql 'account-create' "$variables"
  account_id=$(graphql_output '.data.accountCreate.account.accountId')
  [[ $account_id == "1fd1dd3e-33fe-4ef5-9d58-676ef8d306b5" ]] || exit 1

  variables=$(
    jq -n \
    '{
      "input": {
        "accountId": "78551b96-9c34-46f9-8d5f-c86e4459fcd7",
        "name": "Assets",
        "code": "ASSET",
        "description": "Lava assets (e.g. cash deposits)",
        "normalBalanceType": "DEBIT"
      }
    }'
  )
  exec_graphql 'account-create' "$variables"
  asset_account_id=$(graphql_output '.data.accountCreate.account.accountId')
  [[ $asset_account_id == "78551b96-9c34-46f9-8d5f-c86e4459fcd7" ]] || exit 1
}

@test "cala: transaction template create" {
  create_deposit_tx_template
  deposit_tx_template_id=$(graphql_output '.data.txTemplateCreate.txTemplate.txTemplateId')
  [[ $deposit_tx_template_id == "15f3f5da-034e-40c1-aaff-ab6d01bd44af" ]] || exit 1

  create_withdraw_tx_template
  withdraw_tx_template_id=$(graphql_output '.data.txTemplateCreate.txTemplate.txTemplateId')
  [[ $withdraw_tx_template_id == "fab492ae-2fe4-4fcd-9bf7-cf06eb5f796b" ]] || exit 1
}

@test "cala: post transaction" {
  transaction_id="42847c7f-1972-4448-91b7-652c378760f4"
  variables=$(
    jq -n \
    --arg transaction_id "$transaction_id" \
    '{
      "input": {
        "transactionId": $transaction_id,
        "txTemplateCode": "ACH_CREDIT",
        "params": {
          "account": "1fd1dd3e-33fe-4ef5-9d58-676ef8d306b5",
          "amount": "9.53",
          "effective": "2022-09-21"
        }
      }
    }'
  )
  exec_graphql 'post-transaction' "$variables"
  correlation_id=$(graphql_output '.data.postTransaction.transaction.correlationId')
  [[ $correlation_id == $transaction_id  ]] || exit 1
}

@test "cala: balance for account" {
  variables=$(
    jq -n \
      '{
        "accountId": "1fd1dd3e-33fe-4ef5-9d58-676ef8d306b5",
        "journalId": "822cb59f-ce51-4837-8391-2af3b7a5fc51",
        "currency": "USD",
      }'
  )
  exec_graphql 'account' "$variables"
  balance=$(graphql_output '.data.account.balance.settled.normalBalance.units')
  [[ $balance == "9.53" ]] || exit 1
}

