import {
  CalaLedger,
  NewTxTemplateTransactionValues,
  NewTxTemplateEntryValues,
  NewParamDefinitionValues,
  ParamDataTypeValues,
} from "@galoymoney/cala-ledger";

const storeServerPid = (calaHome: string, pid: number) => {
  const fs = require("fs");
  if (!fs.existsSync(calaHome)) {
    fs.mkdirSync(calaHome);
  }
  try {
    fs.unlinkSync(`${calaHome}/nodejs-example-pid`);
  } catch (e) {
    console.log("No existing PID file to remove");
  }
  fs.writeFileSync(`${calaHome}/nodejs-example-pid`, pid.toString());
};

const main = async () => {
  const pgHost = process.env.PG_HOST || "localhost";
  const pgCon = `postgres://user:password@${pgHost}:5433/pg`;

  console.log("Connecting to CalaLedger...");
  storeServerPid(".cala", process.pid);

  const cala = await CalaLedger.connect({
    pgCon,
    outbox: { enabled: true },
  });
  console.log("CalaLedger connected");

  const account = await cala.accounts().create({
    name: "MY NAME",
    code: "USER:" + Math.floor(Math.random() * 1000),
    metadata: {
      something: "users",
      more: true,
    },
  });
  console.log("Account created", account.id());
  const account2 = await cala.accounts().create({
    name: "MY NAME",
    code: "USER:" + Math.floor(Math.random() * 1000),
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

  while (true) {
    await new Promise((resolve) => setTimeout(resolve, 5000));
  }
};

main();
