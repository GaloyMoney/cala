#!/usr/bin/env bats

load "helpers"

setup_file() {
  start_server
}

teardown_file() {
  stop_server
}

save_json() {
  local filename="$1"
  local json_content="$2"
  if [[ -z "$json_content" || "$json_content" == "null" ]]; then
    echo "JSON content is empty or null, exiting."
    exit 1
  fi
  echo "$json_content" | jq . >"$filename" || {
    echo "Failed to write JSON to $filename"
    exit 1
  }
}

@test "cala: generate variables and responses for the website demo" {
  # creating a journal
  journal_id=$(random_uuid)
  variables=$(jq -n --arg journal_id "$journal_id" '{
    "input": {
      "journalId": $journal_id,
      "name": "General Ledger"
    }
  }')
  exec_graphql 'journal-create' "$variables"
  [[ "$output" != "null" ]] || exit 1
  save_json "${REPO_ROOT}/website/static/gql/variables/journalCreate.json" "$variables"
  save_json "${REPO_ROOT}/website/static/gql/responses/journalCreateResponse.json" "$output"

  # create liability account
  liability_account_id=$(random_uuid)
  variables=$(jq -n --arg liability_account_id "$liability_account_id" '{
    "input": {
      "accountId": $liability_account_id,
      "name": "Alice - Checking",
      "code": ("ALICE.CHECKING-" + $liability_account_id),
      "normalBalanceType": "CREDIT"
    }
  }')
  exec_graphql 'account-create' "$variables"
  [[ "$output" != "null" ]] || exit 1
  save_json "${REPO_ROOT}/website/static/gql/variables/accountCreateChecking.json" "$variables"
  save_json "${REPO_ROOT}/website/static/gql/responses/accountCreateCheckingResponse.json" "$output"

  # create asset account
  asset_account_id=$(random_uuid)
  variables=$(jq -n --arg asset_account_id "$asset_account_id" '{
    "input": {
      "accountId": $asset_account_id,
      "name": "Assets",
      "code": ("ASSET-"+ $asset_account_id),
      "normalBalanceType": "DEBIT"
    }
  }')
  exec_graphql 'account-create' "$variables"
  [[ "$output" != "null" ]] || exit 1
  save_json "${REPO_ROOT}/website/static/gql/variables/accountCreateDebit.json" "$variables"
  save_json "${REPO_ROOT}/website/static/gql/responses/accountCreateDebitResponse.json" "$output"

  # create transaction templates
  deposit_template_id=$(random_uuid)
  withdrawal_template_id=$(random_uuid)
  variables=$(jq -n --arg depositTemplateId "$deposit_template_id" --arg withdrawalTemplateId "$withdrawal_template_id" --arg assetAccountId "$asset_account_id" --arg journalId "$journal_id" '{
    "depositTemplateId": $depositTemplateId,
    "depositTemplateCode": ("DEPOSIT-" + $depositTemplateId),
    "withdrawalTemplateId": $withdrawalTemplateId,
    "withdrawalTemplateCode": ("WITHDRAWAL-" + $withdrawalTemplateId),
    "assetAccountId": ("uuid(\u0027" + $assetAccountId + "\u0027)"),
    "journalId": ("uuid(\u0027" + $journalId + "\u0027)")
  }')
  exec_graphql 'tx-template-create' "$variables"
  save_json "${REPO_ROOT}/website/static/gql/variables/txTemplateCreate.json" "$variables"
  save_json "${REPO_ROOT}/website/static/gql/responses/txTemplateCreateResponse.json" "$output"

  # post transaction
  transaction_id=$(random_uuid)
  variables=$(jq -n --arg transaction_id "$transaction_id" --arg account_id "$liability_account_id" --arg depositTemplateId "$deposit_template_id" '{
    "input": {
      "transactionId": $transaction_id,
      "txTemplateCode": ("DEPOSIT-" + $depositTemplateId),
      "params": {
        "account": $account_id,
        "amount": "9.53",
        "effective": "2022-09-21"
      }
    }
  }')
  exec_graphql 'transaction-post' "$variables"
  correlation_id=$(graphql_output '.data.transactionPost.transaction.correlationId')
  [[ $correlation_id == $transaction_id ]] || exit 1
  save_json "${REPO_ROOT}/website/static/gql/variables/transactionPost.json" "$variables"
  save_json "${REPO_ROOT}/website/static/gql/responses/transactionPostResponse.json" "$output"

  # check account balance
  variables=$(jq -n --arg journalId "$journal_id" --arg accountId "$liability_account_id" '{
    "accountId": $accountId,
    "journalId": $journalId,
    "currency": "USD"
  }')
  exec_graphql 'account-with-balance' "$variables"
  balance=$(graphql_output '.data.account.balance.settled.normalBalance.units')
  [[ $balance == "9.53" ]] || exit 1
  save_json "${REPO_ROOT}/website/static/gql/variables/accountWithBalance.json" "$variables"
  save_json "${REPO_ROOT}/website/static/gql/responses/accountWithBalanceResponse.json" "$output"

  # create account set
  account_set_id=$(random_uuid)
  variables=$(jq -n --arg account_set_id "$account_set_id" --arg journal_id "$journal_id" '{
    "input": {
      "accountSetId": $account_set_id,
      "journalId": $journal_id,
      "name": "Account Set",
      "normalBalanceType": "CREDIT"
    }
  }')

  exec_graphql 'account-set-create' "$variables"
  res=$(graphql_output '.data.accountSetCreate.accountSet.accountSetId')
  [[ "$res" != "null" ]] || exit 1
  save_json "${REPO_ROOT}/website/static/gql/variables/accountSetCreate.json" "$variables"
  save_json "${REPO_ROOT}/website/static/gql/responses/accountSetCreateResponse.json" "$output"

  # add account to account set
  variables=$(jq -n --arg account_set_id "$account_set_id" --arg member_id "$liability_account_id" '{
    "input": {
      "accountSetId": $account_set_id,
      "memberId": $member_id,
      "memberType": "ACCOUNT"
    }
  }')
  exec_graphql 'add-to-account-set' "$variables"
  balance=$(graphql_output '.data.addToAccountSet.accountSet.balance.settled.normalBalance.units')
  [[ $balance == "9.53" ]] || exit 1
  save_json "${REPO_ROOT}/website/static/gql/variables/addToAccountSet.json" "$variables"
  save_json "${REPO_ROOT}/website/static/gql/responses/addToAccountSetResponse.json" "$output"

  # balance check for account set
  variables=$(jq -n --arg journalId "$journal_id" --arg accountSetId "$account_set_id" '{
    "accountSetId": $accountSetId,
    "journalId": $journalId,
    "currency": "USD"
  }')
  exec_graphql 'account-set-with-balance' "$variables"
  balance=$(graphql_output '.data.accountSet.balance.settled.normalBalance.units')
  [[ $balance == "9.53" ]] || exit 1
  save_json "${REPO_ROOT}/website/static/gql/variables/accountSetWithBalance.json" "$variables"
  save_json "${REPO_ROOT}/website/static/gql/responses/accountSetWithBalanceResponse.json" "$output"
}
