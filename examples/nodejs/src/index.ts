import { CalaLedger } from "@galoymoney/cala-ledger";

const main = async () => {
  const pgHost = process.env.PG_HOST || "localhost";
  const pgCon = `postgres://user:password@${pgHost}:5432/pg`;

  const cala = await CalaLedger.connect({ pgCon, outbox: { enabled: true } })
  console.log("CalaLedger connected");

  const account = await cala.accounts().create({
    name: "MY NAME",
    code: "USERS_ONE",
    metadata: {
      "something": "users",
      "more": true
    }
  })
  console.log("Account created", account.id());
  const account2 = await cala.accounts().create({
    name: "MY NAME",
    code: "USERS_TWO",
    metadata: {
      "something": "users",
      "more": true
    }
  })

  console.log("Account created", account2.id());

  let result = await cala.accounts().list({ first: 1 });
  console.log("First accounts: ", result);

  result = await cala.accounts().list({ first: 1, after: result.endCursor });
  console.log("Next accounts: ", result);

  const journal = await cala.journals().create({
    name: "MY JOURNAL",
    description: "MY DESCRIPTION",
  })

  console.log("Journal Created", journal.id());
}

main()
