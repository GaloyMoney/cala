import {
  CalaLedger,
  NewTxTemplateTransactionValues,
  NewTxTemplateEntryValues,
  NewParamDefinitionValues,
  ParamDataTypeValues,
} from "@galoymoney/cala-ledger";

const main = async () => {
  const pgHost = process.env.PG_HOST || "localhost";
  const pgCon = `postgres://user:password@${pgHost}:5433/pg`;

  const cala = await CalaLedger.connect({
    pgCon,
    outbox: { enabled: true, listenPort: 2258 },
  });
  console.log("CalaLedger connected");

  const account = await cala.accounts().create({
    name: "MY NAME",
    code: "USERS_ONE",
    metadata: {
      something: "users",
      more: true,
    },
  });
  console.log("Account created", account.id());
  const account2 = await cala.accounts().create({
    name: "MY NAME",
    code: "USERS_TWO",
    metadata: {
      something: "users",
      more: true,
    },
  });

  console.log("Account created", account2.id());

  let result = await cala.accounts().list({ first: 1 });
  console.log("First accounts: ", result);

  result = await cala.accounts().list({ first: 1, after: result.endCursor });
  console.log("Next accounts: ", result);

  const journal = await cala.journals().create({
    name: "MY JOURNAL",
    description: "MY DESCRIPTION",
    code: "MY_JOURNAL",
  });

  console.log("Journal Created", journal.id());

  const recordDepositDrEntry: NewTxTemplateEntryValues = {
    entryType: "'RECORD_DEPOSIT_DR'",
    currency: "params.currency",
    accountId: "params.deposit_omnibus_account_id",
    direction: "DEBIT",
    layer: "SETTLED",
    units: "params.amount",
  };

  const recordDepositCrEntry: NewTxTemplateEntryValues = {
    entryType: "'RECORD_DEPOSIT_CR'",
    currency: "params.currency",
    accountId: "params.credit_account_id",
    direction: "CREDIT",
    layer: "SETTLED",
    units: "params.amount",
  };

  const txInput: NewTxTemplateTransactionValues = {
    journalId: "params.journal_id",
    effective: "params.effective",
    metadata: "params.meta",
    description: "'Record a deposit'",
  };

  const txParams: NewParamDefinitionValues[] = [
    {
      name: "journal_id",
      type: ParamDataTypeValues.Uuid,
    },
    {
      name: "currency",
      type: ParamDataTypeValues.String,
    },
    {
      name: "amount",
      type: ParamDataTypeValues.Decimal,
    },
    {
      name: "deposit_omnibus_account_id",
      type: ParamDataTypeValues.Uuid,
    },
    {
      name: "credit_account_id",
      type: ParamDataTypeValues.Uuid,
    },
    {
      name: "effective",
      type: ParamDataTypeValues.Date,
    },
    {
      name: "meta",
      type: ParamDataTypeValues.Json,
    },
  ];

  const txTemplate = await cala.txTemplates().create({
    code: "RECORD_DEPOSIT",
    description: "Record deposit transaction",
    entries: [recordDepositDrEntry, recordDepositCrEntry],
    transaction: txInput,
    params: txParams,
  });

  console.log(
    "Tx Template Created",
    txTemplate.values().id,
    txTemplate.values().code,
  );

  const retrievedTxTemplate = await cala
    .txTemplates()
    .findByCode("RECORD_DEPOSIT");

  console.log("Retrieved Tx Template", retrievedTxTemplate.values());
};

main();
