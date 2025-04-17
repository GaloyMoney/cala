#!/usr/bin/env bats

load "helpers"

setup_file() {
  make reset-tf-state run-tf || true
}

@test "add-interest-account: can add account" {
  interest_account_id=$(random_uuid)
  variables=$(
    jq -n \
    --arg interest_account_id "$interest_account_id" \
    '{
      "interestAccountId": $interest_account_id,
      "interestAccountName": ("Interest Income #" + $interest_account_id),
      "interestAccountCode": ("INTEREST_INCOME." + $interest_account_id),
      "interestRevenueControlAccountSetId": "00000000-0000-0000-0000-140000000001"
    }'
  )
  exec_graphql 'add-interest-account' "$variables"
  graphql_output
  id=$(graphql_output '.data.interest.account.accountId')
  [[ "$id" == "$interest_account_id" ]] || exit 1
}
