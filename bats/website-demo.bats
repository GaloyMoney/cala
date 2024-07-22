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
  gql_request='journal-create'
  exec_graphql "$gql_request" "$variables"
  [[ "$output" != "null" ]] || exit 1
  save_json "${REPO_ROOT}/website/static/gql/variables/journalCreate.json" "$variables"
  save_json "${REPO_ROOT}/website/static/gql/responses/journalCreateResponse.json" "$output"
  cp "${REPO_ROOT}/bats/gql/${gql_request}.gql" "${REPO_ROOT}/website/static/gql/"

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
  gql_request='account-create'
  exec_graphql "$gql_request" "$variables"
  [[ "$output" != "null" ]] || exit 1
  save_json "${REPO_ROOT}/website/static/gql/variables/accountCreateChecking.json" "$variables"
  save_json "${REPO_ROOT}/website/static/gql/responses/accountCreateCheckingResponse.json" "$output"
  cp "${REPO_ROOT}/bats/gql/${gql_request}.gql" "${REPO_ROOT}/website/static/gql/"

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
  exec_graphql "$gql_request" "$variables"
  [[ "$output" != "null" ]] || exit 1
  save_json "${REPO_ROOT}/website/static/gql/variables/accountCreateDebit.json" "$variables"
  save_json "${REPO_ROOT}/website/static/gql/responses/accountCreateDebitResponse.json" "$output"
  cp "${REPO_ROOT}/bats/gql/${gql_request}.gql" "${REPO_ROOT}/website/static/gql/"

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
  gql_request='tx-template-create'
  exec_graphql "$gql_request" "$variables"
  save_json "${REPO_ROOT}/website/static/gql/variables/txTemplateCreate.json" "$variables"
  save_json "${REPO_ROOT}/website/static/gql/responses/txTemplateCreateResponse.json" "$output"
  cp "${REPO_ROOT}/bats/gql/${gql_request}.gql" "${REPO_ROOT}/website/static/gql/"

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
  gql_request='transaction-post'
  exec_graphql "$gql_request" "$variables"
  correlation_id=$(graphql_output '.data.transactionPost.transaction.correlationId')
  [[ $correlation_id == $transaction_id ]] || exit 1
  save_json "${REPO_ROOT}/website/static/gql/variables/transactionPost.json" "$variables"
  save_json "${REPO_ROOT}/website/static/gql/responses/transactionPostResponse.json" "$output"
  cp "${REPO_ROOT}/bats/gql/${gql_request}.gql" "${REPO_ROOT}/website/static/gql/"

  # check account balance
  variables=$(jq -n --arg journalId "$journal_id" --arg accountId "$liability_account_id" '{
    "accountId": $accountId,
    "journalId": $journalId,
    "currency": "USD"
  }')
  gql_request='account-with-balance'
  exec_graphql "$gql_request" "$variables"
  balance=$(graphql_output '.data.account.balance.settled.normalBalance.units')
  [[ $balance == "9.53" ]] || exit 1
  save_json "${REPO_ROOT}/website/static/gql/variables/accountWithBalance.json" "$variables"
  save_json "${REPO_ROOT}/website/static/gql/responses/accountWithBalanceResponse.json" "$output"
  cp "${REPO_ROOT}/bats/gql/${gql_request}.gql" "${REPO_ROOT}/website/static/gql/"

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
  gql_request='account-set-create'
  exec_graphql "$gql_request" "$variables"
  res=$(graphql_output '.data.accountSetCreate.accountSet.accountSetId')
  [[ "$res" != "null" ]] || exit 1
  save_json "${REPO_ROOT}/website/static/gql/variables/accountSetCreate.json" "$variables"
  save_json "${REPO_ROOT}/website/static/gql/responses/accountSetCreateResponse.json" "$output"
  cp "${REPO_ROOT}/bats/gql/${gql_request}.gql" "${REPO_ROOT}/website/static/gql/"

  # add account to account set
  variables=$(jq -n --arg account_set_id "$account_set_id" --arg member_id "$liability_account_id" '{
    "input": {
      "accountSetId": $account_set_id,
      "memberId": $member_id,
      "memberType": "ACCOUNT"
    }
  }')
  gql_request='add-to-account-set'
  exec_graphql "$gql_request" "$variables"
  balance=$(graphql_output '.data.addToAccountSet.accountSet.balance.settled.normalBalance.units')
  [[ $balance == "9.53" ]] || exit 1
  save_json "${REPO_ROOT}/website/static/gql/variables/addToAccountSet.json" "$variables"
  save_json "${REPO_ROOT}/website/static/gql/responses/addToAccountSetResponse.json" "$output"
  cp "${REPO_ROOT}/bats/gql/${gql_request}.gql" "${REPO_ROOT}/website/static/gql/"

  # balance check for account set
  variables=$(jq -n --arg journalId "$journal_id" --arg accountSetId "$account_set_id" '{
    "accountSetId": $accountSetId,
    "journalId": $journalId,
    "currency": "USD"
  }')
  gql_request='account-set-with-balance'
  exec_graphql "$gql_request" "$variables"
  balance=$(graphql_output '.data.accountSet.balance.settled.normalBalance.units')
  [[ $balance == "9.53" ]] || exit 1
  save_json "${REPO_ROOT}/website/static/gql/variables/accountSetWithBalance.json" "$variables"
  save_json "${REPO_ROOT}/website/static/gql/responses/accountSetWithBalanceResponse.json" "$output"
  cp "${REPO_ROOT}/bats/gql/${gql_request}.gql" "${REPO_ROOT}/website/static/gql/"
}
