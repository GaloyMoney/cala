import { init } from "@galoymoney/cala-ledger";

const main = async () => {
  const pgHost = process.env.PG_HOST || "localhost";
  const pgCon = `postgres://user:password@${pgHost}:5432/pg`;

  init({pgCon})
  console.log("Hello World!");
}

main()
