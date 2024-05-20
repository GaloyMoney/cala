create_deposit_tx_template_variables() {
  jq -n \
  '{
    "input": {
      "txTemplateId": "15f3f5da-034e-40c1-aaff-ab6d01bd44af",
      "code": "ACH_CREDIT",
      "description": "An ACH credit into a customer account.",
      "params": [
        {
          "name": "account",
          "type": "UUID",
          "description": "Deposit account ID."
        },
        {
          "name": "amount",
          "type": "DECIMAL",
          "description": "Amount with decimal, e.g. `1.23`."
        },
        {
          "name": "effective",
          "type": "DATE",
          "description": "Effective date for transaction."
        }
      ],
      "txInput": {
        "journalId": "uuid(\u0027822cb59f-ce51-4837-8391-2af3b7a5fc51\u0027)",
        "effective": "params.effective"
      },
      "entries": [
        {
          "accountId": "uuid(\u002778551b96-9c34-46f9-8d5f-c86e4459fcd7\u0027)",
          "units": "params.amount",
          "currency": "\u0027USD\u0027",
          "entryType": "\u0027ACH_DR\u0027",
          "direction": "DEBIT",
          "layer": "SETTLED"
        },
        {
          "accountId": "params.account",
          "units": "params.amount",
          "currency": "\u0027USD\u0027",
          "entryType": "\u0027ACH_CR\u0027",
          "direction": "CREDIT",
          "layer": "SETTLED"
        }
      ]
    }
  }'
}

create_deposit_tx_template() {
  exec_graphql 'tx-template-create' "$(create_deposit_tx_template_variables)"
}

create_withdraw_tx_template_variables() {
  jq -n \
  '{
    "input": {
      "txTemplateId": "fab492ae-2fe4-4fcd-9bf7-cf06eb5f796b",
      "code": "ACH_DEBIT",
      "description": "An ACH debit into a customer account.",
      "params": [
        {
          "name": "account",
          "type": "UUID",
          "description": "Withdraw account ID."
        },
        {
          "name": "amount",
          "type": "DECIMAL",
          "description": "Amount with decimal, e.g. `1.23`."
        },
        {
          "name": "effective",
          "type": "DATE",
          "description": "Effective date for transaction."
        }
      ],
      "txInput": {
        "journalId": "uuid(\u0027822cb59f-ce51-4837-8391-2af3b7a5fc51\u0027)",
        "effective": "params.effective"
      },
      "entries": [
        {
          "accountId": "uuid(\u002778551b96-9c34-46f9-8d5f-c86e4459fcd7\u0027)",
          "units": "params.amount",
          "currency": "\u0027USD\u0027",
          "entryType": "\u0027ACH_CR\u0027",
          "direction": "CREDIT",
          "layer": "SETTLED"
        },
        {
          "accountId": "params.account",
          "units": "params.amount",
          "currency": "\u0027USD\u0027",
          "entryType": "\u0027ACH_DR\u0027",
          "direction": "DEBIT",
          "layer": "SETTLED"
        }
      ]
    }
  }'
}

create_withdraw_tx_template() {
  exec_graphql 'tx-template-create' "$(create_withdraw_tx_template_variables)"
}

