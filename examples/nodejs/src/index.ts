import { CalaLedger } from "@galoymoney/cala-ledger";

const main = async () => {
  const pgHost = process.env.PG_HOST || "localhost";
  const pgCon = `postgres://user:password@${pgHost}:5432/pg`;

  const cala = await CalaLedger.connect({pgCon, outbox: { enabled: true }})
  console.log("CalaLedger connected");

  const accountId = await cala.accounts().create({
    name: "MY NAME",
    code: "USERS_ONE",
    metadata: {
      "something": "users",
      "more": true
    }
  })
  console.log("Account created", accountId);
  const accountId2 = await cala.accounts().create({
    name: "MY NAME",
    code: "USERS_TWO",
    metadata: {
      "something": "users",
      "more": true
    }
  })
  console.log("Account created", accountId2);
  const accounts = await cala.accounts().list({first: 10});
  console.log("Accounts: ", accounts)
  console.log("Awaiting server");
  await cala.awaitOutboxServer();
}

main()
