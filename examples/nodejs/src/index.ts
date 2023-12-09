import { CalaLedger } from "@galoymoney/cala-ledger";

const main = async () => {
  const pgHost = process.env.PG_HOST || "localhost";
  const pgCon = `postgres://user:password@${pgHost}:5432/pg`;

  const cala = await CalaLedger.connect({pgCon})
  console.log("CalaLedger connected");

  const account_id = await cala.accounts().create({
    name: "USERS_ONE",
    code: "USERS_ONE",
    metadata: {
      "something": "users",
      "more": true
    }
  })
  console.log("Account created", account_id);
}

main()
