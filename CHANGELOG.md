# [cala release v0.1.5](https://github.com/GaloyMoney/cala/releases/tag/0.1.5)


### Features

- Atomic mutations (#78)
- Account set member (#76)
- AccountSet (#74)
- Transaction_by_external_id
- Support CEL packages (#72)

### Miscellaneous Tasks

- Rename account -> account set in expect (#81)
- Remove dummy output
- Record account set balances (#80)
- Expose created / modified (#73)
- Add transaction(id)

### Refactor

- Atomic mutations for tx_template_create and post_transaction (#79)
- Use atomic operation for creating entities (#77)
- Add entry_ids to TransactionValues
- Rename tx -> db

# [cala release v0.1.4](https://github.com/GaloyMoney/cala/releases/tag/0.1.4)


### Miscellaneous Tasks

- Add tracing to balance/mod.rs

# [cala release v0.1.3](https://github.com/GaloyMoney/cala/releases/tag/0.1.3)


### Bug Fixes

- Fmt
- Query correct table in journals.find_all

### Features

- Add tx template find-by-code query (#71)

### Miscellaneous Tasks

- Output job errors
- Gen lowercase uuid in bats helpers
- Add version to account (#70)
- Add version to TxTemplate
- Fix e2e test (#69)
- Bump prost from 0.12.4 to 0.12.6 (#66)
- Bump napi-derive from 2.16.4 to 2.16.5 (#67)
- Bump thiserror from 1.0.60 to 1.0.61 (#68)
- Bump anyhow from 1.0.83 to 1.0.86 (#65)
- Expose tx_template query
- Fix default TracingConfig
- More instrumentation
- Add journal lookup

### Testing

- Initial e2e test setup (#63)

# [cala release v0.1.2](https://github.com/GaloyMoney/cala/releases/tag/0.1.2)


### Miscellaneous Tasks

- No need for sqlx::Type
- Add FromRow to GenericEvent
- Make account / balance optional
- Return Option for queries

# [cala release v0.1.1](https://github.com/GaloyMoney/cala/releases/tag/0.1.1)


### Miscellaneous Tasks

- Add symlink to proto to include proto files in crate

# [cala release v0.1.0](https://github.com/GaloyMoney/cala/releases/tag/0.1.0)


### Bug Fixes

- Tracing
- Typo
- Cala outbox mutation name
- Make persist data source aware
- Workspace = true in cala-tracing
- Load +1 import-jobs in list
- Clippy
- Remove redundant comma

### Documentation

- Add basic readme

### Features

- Add entry entity (#51)
- Add transaction (#38)
- Account_create gql layer (#47)
- Expose generic job entity (#44)
- Gql layer for tx_template (#45)
- Add tx_template (#35)
- Add cel parser and interpreter (#30)
- Setup tracing (#16)

### Miscellaneous Tasks

- Balance gel layer (#64)
- CodeAlreadyExists error
- Explicit ExternalIdAlreadyExists
- Add accountByExternalId
- Post_transaction gql boilerplate (#59)
- Check job  type when spawning
- Pass server id as ref (#62)
- Add service_instance_id to tracing
- Bump pg (#61)
- Reference latest_entry_id from cala_balance_history (#60)
- Port post-transaction from sqlx-ledger (#57)
- Bump serde from 1.0.201 to 1.0.202 (#52)
- Add balance.rs (#53)
- Wrap core registration
- Expose hook for job registration
- Rename entity id's in gql layer (#48)
- Remove dead file
- Bump async-graphql-axum from 7.0.3 to 7.0.5 (#46)
- Extension boilerplate (#43)
- Bump serde_json from 1.0.116 to 1.0.117 (#32)
- Bump serde from 1.0.200 to 1.0.201 (#33)
- Persist job state (#41)
- Bump async-graphql from 7.0.3 to 7.0.5 (#37)
- Fmt
- Complete outbox sync (#39)
- Complete OutboxListener implementation (#34)
- Bump napi-derive from 2.16.3 to 2.16.4 (#26)
- Bump thiserror from 1.0.59 to 1.0.60 (#27)
- Bump anyhow from 1.0.82 to 1.0.83 (#29)
- Bump napi from 2.16.4 to 2.16.6 (#31)
- Create outbox client in import job
- Job execution (#24)
- Bump flake (#23)
- Update tonic-build requirement from 0.10.2 to 0.11.0 (#21)
- Expose list import jobs
- Create ImportJob via graphql
- Bump rust
- Bump tonic
- Update base64 requirement from 0.21.5 to 0.22.1 (#11)
- Update derive_builder requirement from 0.12.0 to 0.20.0 (#8)
- Add ImportJob boilerplate
- Update entity framework from galoy
- Sync tracing from galoy
- Typesafe Tag
- Generate journal gql types (#5)
- Expose journal values
- Return journal in gql layer
- Impl default for Status
- Create journals using gql
- Create journals from cala-nodejs (#3)
- Usecase in mod.rs for journal
- Expand rust example
- Initialising journal
- Remove federation directives
- Wire accounts query e2e
- Add Cursor for accounts
- Fix pagination for accounts
- Paginated accounts
- Fix sequence index
- Some boilerplate for querying
- Cala-server boilerplate
- Move extract_grpc_tracing to cala-tracing
- Cala-server boilerplate
- Add awaitOutboxServer (hacky)
- Start server
- Add cala-ledger-outbox-client
- Outbox server boilerplate
- Remove augmentation
- Persist outbox events e2e
- Pass new events to outbox
- Outbox boilerplate
- Add metadata to napi account
- Add account creation to examples/nodejs
- Accounts boilerplate
- Error handling in nodejs
- Actually use ledger in nodejs bindings
- Examples/nodejs e2e
- Nodejs boilerplate
- CalaLedgerConfig / migrations
- Cala-ledger boilerplate
- Add flake.nix

### Refactor

- Remove account 'tags' attribute (#58)
- Registry addInitializer accepts type
- Job init need not be async
- Cleaner external extension support
- Job execution (#36)
- Move query out of cala-core-types
- Consistently return entity from create
- Return journal_values
- Consistent singular module name
- Restructure account fields / indexes
- Re-export cala_types where useful
- Move outbox event to core-types
- Extract core-types

### Testing

- Post_transaction.rs boilerplate (#54)
- Improve bats idempotency (#42)
- Complete assertion in example (#40)
- Bats boilerplate (#17)
