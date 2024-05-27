#!/usr/bin/env bats

load "helpers"

setup_file() {
  start_server
}

teardown_file() {
  stop_server
}

@test "cala: check balance updates for an account set" {
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

  # create assets and liabilities accounts
  liability_account_id=$(random_uuid)
  variables=$(
    jq -n \
    --arg liability_account_id "$liability_account_id" \
    --arg account_set_id "$account_set_id" \
    '{
      "input": {
        "accountId": $liability_account_id,
        "name": "Alice - Checking",
        "code": ("ALICE.CHECKING-" + $liability_account_id),
        "accountSetIds": [$account_set_id],
        "normalBalanceType": "CREDIT"
      }
    }'
  )
  exec_graphql 'account-create' "$variables"
  output=$(graphql_output '.data.accountCreate.account.accountId')
  [[ "$output" != "null" ]] || exit 1

  asset_account_id=$(random_uuid)
  variables=$(
    jq -n \
    --arg asset_account_id "$asset_account_id" \
    '{
      "input": {
        "accountId": $asset_account_id,
        "name": "Assets",
        "code": ("ASSET-"+ $asset_account_id),
        "normalBalanceType": "DEBIT"
      }
    }'
  )
  exec_graphql 'account-create' "$variables"
  output=$(graphql_output '.data.accountCreate.account.accountId')
  [[ "$output" != "null" ]] || exit 1

  # create tx templates
  deposit_template_id=$(random_uuid)
  withdrawal_template_id=$(random_uuid)
  variables=$(jq -n \
  --arg depositTemplateId "$deposit_template_id" \
  --arg withdrawalTemplateId "$withdrawal_template_id" \
  --arg assetAccountId "$asset_account_id" \
  --arg journalId "$journal_id" \
  '{
    "depositTemplateId": $depositTemplateId,
    "depositTemplateCode": ("DEPOSIT-" + $depositTemplateId),
    "withdrawalTemplateId": $withdrawalTemplateId,
    "withdrawalTemplateCode": ("WITHDRAWAL-" + $withdrawalTemplateId),
    "assetAccountId": ("uuid(\u0027" + $assetAccountId + "\u0027)"),
    "journalId": ("uuid(\u0027" + $journalId + "\u0027)")
  }')
  exec_graphql 'tx-template-create' "$variables"

  # post transaction
  transaction_id=$(random_uuid)
  variables=$(
    jq -n \
    --arg transaction_id "$transaction_id" \
    --arg account_id "$liability_account_id" \
    --arg depositTemplateId "$deposit_template_id" \
    '{
      "input": {
        "transactionId": $transaction_id,
        "txTemplateCode": ("DEPOSIT-" + $depositTemplateId),
        "params": {
          "account": $account_id,
          "amount": "9.53",
          "effective": "2022-09-21"
        }
      }
    }'
  )
  exec_graphql 'post-transaction' "$variables"

  variables=$(jq -n \
    --arg journalId "$journal_id" \
    --arg accountSetId "$account_set_id" \
    '{
      "accountSetId": $accountSetId,
      "journalId": $journalId,
      "currency": "USD"
    }'
  )
  exec_graphql 'account-set-with-balance' "$variables"
  balance=$(graphql_output '.data.account.balance.settled.normalBalance.units')
  [[ $balance == "9.53" ]] || exit 1

}
