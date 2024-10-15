#!/usr/bin/env bats

load "helpers"

setup_file() {
  start_server
}

teardown_file() {
  stop_server
}

@test "cala: create velocity control and post transaction with limits" {
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
  [[ "$output" != "null" ]] || exit 1

  # create accounts
  
  sender_account_id=$(random_uuid)
  variables=$(
    jq -n \
    --arg sender_account_id "$sender_account_id" \
    '{
      "input": {
        "accountId": $sender_account_id,
        "name": "Sender - Checking",
        "code": ("SENDER.CHECKING-" + $sender_account_id),
        "normalBalanceType": "CREDIT"
      }
    }'
  )
  exec_graphql 'account-create' "$variables"
  output=$(graphql_output '.data.accountCreate.account.accountId')
  [[ "$output" != "null" ]] || exit 1

  recipient_account_id=$(random_uuid)
  variables=$(
    jq -n \
    --arg recipient_account_id "$recipient_account_id" \
    '{
      "input": {
        "accountId": $recipient_account_id,
        "name": "Recipient - Checking",
        "code": ("RECIPIENT.CHECKING-" + $recipient_account_id),
        "normalBalanceType": "DEBIT"
      }
    }'
  )
  exec_graphql 'account-create' "$variables"
  output=$(graphql_output '.data.accountCreate.account.accountId')
  [[ "$output" != "null" ]] || exit 1

  # create velocity limits
  withdrawal_limit_id=$(random_uuid)
  variables=$(
    jq -n \
    --arg velocityLimitId "$withdrawal_limit_id" \
    '{
      "input": {
        "velocityLimitId": $velocityLimitId,
        "name": ("Withdrawal-" + $velocityLimitId),
        "description": "Test withdrawal limit",
        "window": [],
        "currency": null,
        "limit": {
          "timestampSource": null,
          "balance": [{
            "limitType": "AVAILABLE",
            "layer": "SETTLED",
            "amount": "params.deposit_limit",
            "normalBalanceType": "DEBIT",
          }]
        },
        "params": [{
          "name": "withdrawal_limit",
          "type": "DECIMAL",
          "default": null,
          "description": null
        }]
      }
    }'
  )
  exec_graphql 'velocity-limit-create' "$variables"
  velocity_limit_id=$(graphql_output '.data.velocityLimitCreate.velocityLimit.velocityLimitId')
  [[ "$velocity_limit_id" == "$withdrawal_limit_id" ]] || exit 1

  deposit_limit_id=$(random_uuid)
  variables=$(
    jq -n \
    --arg velocityLimitId "$deposit_limit_id" \
    '{
      "input": {
        "velocityLimitId": $velocityLimitId,
        "name": ("Deposit-" + $velocityLimitId),
        "description": "Test deposit limit",
        "window": [],
        "currency": null,
        "limit": {
          "timestampSource": null,
          "balance": [{
            "limitType": "AVAILABLE",
            "layer": "SETTLED",
            "amount": "params.deposit_limit",
            "normalBalanceType": "DEBIT",
          }]
        },
        "params": [{
          "name": "deposit_limit",
          "type": "DECIMAL",
          "default": null,
          "description": null
        }]
      }
    }'
  )
  exec_graphql 'velocity-limit-create' "$variables"
  velocity_limit_id=$(graphql_output '.data.velocityLimitCreate.velocityLimit.velocityLimitId')
  [[ "$velocity_limit_id" == "$deposit_limit_id" ]] || exit 1


  # create velocity control
  control_id=$(random_uuid)
  variables=$(
    jq -n \
    --arg control_id "$control_id" \
    '{
      "input": {
        "velocityControlId": $control_id,
        "name": ("Velocity Control-" + $control_id),
        "description": "Test velocity control",
        "enforcement": {
          "velocityEnforcementAction": "REJECT"
        }
      }
    }'
  )
  exec_graphql 'velocity-control-create' "$variables"
  velocity_control_id=$(graphql_output '.data.velocityControlCreate.velocityControl.velocityControlId')
  [[ "$velocity_control_id" == "$control_id" ]] || exit 1

  # attach limits to control
  variables=$(
    jq -n \
    --arg velocity_control_id "$control_id" \
    --arg velocity_limit_id "$withdrawal_limit_id" \
    '{
      "input": {
        "velocityControlId": $velocity_control_id,
        "velocityLimitId": $velocity_limit_id
      }
    }'
  )
  exec_graphql 'velocity-control-add-limit' "$variables"
  echo $(graphql_output) | jq .
  n_limits=$(graphql_output '.data.velocityControlAddLimits.velocityControl.limits') | jq length
  [[ $n_limits == 1 ]] || exit 1

  variables=$(
    jq -n \
    --arg velocity_control_id "$control_id" \
    --arg velocity_limit_id "$deposit_limit_id" \
    '{
      "input": {
        "velocityControlId": $velocity_control_id,
        "velocityLimitId": $velocity_limit_id
      }
    }'
  )
  exec_graphql 'velocity-control-add-limit' "$variables"
  echo $(graphql_output) | jq . 
  n_limits=$(graphql_output '.data.velocityControlAddLimits.velocityControl.limits') | jq length
  [[ $n_limits == 2 ]] || exit 1

  # attach control to sender account
  variables=$(
    jq -n \
    --arg control_id "$control_id" \
    --arg sender_account_id "$sender_account_id" \
    '{
      "input": {
        "controlId": $control_id,
        "accountId": $sender_account_id,
        "params": {
          "withdrawal_limit": "100.00",
          "deposit_limit": "100.00"
        }
      }
    }'
  )
  exec_graphql 'velocity-control-attach-account' "$variables"
  output=$(graphql_output '.data.velocityControlAttachAccount.velocityControl.controlId')
  [[ "$output" != "null" ]] || exit 1

  # post transaction
  transaction_id=$(random_uuid)
  variables=$(
    jq -n \
    --arg transaction_id "$transaction_id" \
    --arg sender_account_id "$sender_account_id" \
    --arg recipient_account_id "$recipient_account_id" \
    --arg amount "50.00" \
    '{
      "input": {
        "transactionId": $transaction_id,
        "sender": $sender_account_id,
        "recipient": $recipient_account_id,
        "amount": $amount
      }
    }'
  )
  exec_graphql 'transaction-post' "$variables"
  correlation_id=$(graphql_output '.data.transactionPost.transaction.correlationId')
  [[ $correlation_id == $transaction_id  ]] || exit 1

  # check balance
  variables=$(jq -n \
    --arg journalId "$journal_id" \
    --arg accountId "$sender_account_id" \
    '{
      "accountId": $accountId,
      "journalId": $journalId,
      "currency": "USD"
    }'
  )
  exec_graphql 'account-with-balance' "$variables"
  balance=$(graphql_output '.data.account.balance.settled.normalBalance.units')
  [[ $balance == "50.00" ]] || exit 1
}
