# [cala release v0.11.4](https://github.com/GaloyMoney/cala/releases/tag/0.11.4)


### Miscellaneous Tasks

- Bump es-entity / job

# [cala release v0.11.3](https://github.com/GaloyMoney/cala/releases/tag/0.11.3)


### Miscellaneous Tasks

- Bump es-entity / job

### Testing

- Increase timeout for job

# [cala release v0.11.2](https://github.com/GaloyMoney/cala/releases/tag/0.11.2)


### Miscellaneous Tasks

- Do not report err in trace

# [cala release v0.11.1](https://github.com/GaloyMoney/cala/releases/tag/0.11.1)


### Bug Fixes

- Do not include_job_migration (its already included)

### Miscellaneous Tasks

- Add account set find_in_op (#544)

# [cala release v0.11.0](https://github.com/GaloyMoney/cala/releases/tag/0.11.0)


### Bug Fixes

- Do not include job migration explicitly in migrate.rs

### Miscellaneous Tasks

- Update velocity test to check for metadata presence (#543)
- Bump job crate
- Bump chrono from 0.4.41 to 0.4.42 (#542)
- Update velocity test to check for metadata presence
- Bump es-entity / remove event-context feature

### Refactor

- Use job crate (#541)

# [cala release v0.10.0](https://github.com/GaloyMoney/cala/releases/tag/0.10.0)


### Bug Fixes

- Consistent order of velocity balance locks (#533)

### Performance

- [**breaking**] Cache latest_values and improve balance insert (#538)
- Cache latest_values in current_balances (#536)
- Use op in prepare_template (#535)
- Don't re-calculate same day effective balance (#534)

# [cala release v0.9.0](https://github.com/GaloyMoney/cala/releases/tag/0.9.0)


### Bug Fixes

- Avoid deadlock in post_transaction by using consistent order of lock aquistion (#532)

### Features

- Add Date and Timestamp cel comparison handling (#531)
- [**breaking**] Cache VelocityContextAccountValues values on accounts table (#529)

### Miscellaneous Tasks

- Bump regex from 1.11.1 to 1.11.2 (#526)
- Add support for 'has()' macro (#517)
- Bump uuid from 1.17.0 to 1.18.1 (#527)
- Bump clap from 4.5.46 to 4.5.47 (#528)

### Performance

- Add some benchmarking (#530)

### Testing

- Add velocity date condition on account set case (#507)

# [cala release v0.8.1](https://github.com/GaloyMoney/cala/releases/tag/0.8.1)


### Bug Fixes

- Tracing with updated libs

### Miscellaneous Tasks

- Add metadata accessor to transaction
- Bump es-entity (#525)
- Bump async-trait from 0.1.88 to 0.1.89 (#522)
- Bump anyhow from 1.0.98 to 1.0.99 (#523)
- Bump rand from 0.9.1 to 0.9.2 (#524)
- Bump schemars from 1.0.3 to 1.0.4 (#521)
- Bump clap from 4.5.42 to 4.5.46 (#520)
- Bump tokio from 1.46.0 to 1.47.1 (#519)
- Bump axum (#518)
- Bump thiserror from 2.0.12 to 2.0.16 (#513)
- Bump serde_json from 1.0.140 to 1.0.143 (#514)
- Bump tracing-subscriber from 0.3.19 to 0.3.20 (#510)

# [cala release v0.8.0](https://github.com/GaloyMoney/cala/releases/tag/0.8.0)


### Features

- [**breaking**] Event-context feature (#508)
- Implement account-set-aware velocities (#502)

### Miscellaneous Tasks

- Improve deps lifecycle make commands (#506)

### Performance

- Use uuid v7 (#489)

### Refactor

- Add limit context to velocity balance function names (#505)
- Clean up velocity `new_snapshot` logic (#503)

# [cala release v0.7.0](https://github.com/GaloyMoney/cala/releases/tag/0.7.0)


### Documentation

- Api reference and website examples update to e733b14069effe7ec606658ea042c2025734017b

### Miscellaneous Tasks

- Bump flake + clip 1.89 (#494)

### Refactor

- [**breaking**] Extract es entity (#491)
- Use unnest in balance repos (#488)
- Iterate once (#487)
- Use unnest to mitigate large arguments (#486)

# [cala release v0.6.12](https://github.com/GaloyMoney/cala/releases/tag/0.6.12)


### Documentation

- Api reference and website examples update to 1899a7926041f2f82ff95a982e8833dded40f7a3

### Miscellaneous Tasks

- Improve tracing for insert_new_snapshot (#485)

# [cala release v0.6.11](https://github.com/GaloyMoney/cala/releases/tag/0.6.11)


### Refactor

- Pass voiding_tx_id as param to void_transaction (#479)

# [cala release v0.6.10](https://github.com/GaloyMoney/cala/releases/tag/0.6.10)


### Documentation

- Api reference and website examples update to 08b9a5f838b0de6c075d6360fe49bf5a50c2caa5

### Miscellaneous Tasks

- Add timeout to sim-time

# [cala release v0.6.9](https://github.com/GaloyMoney/cala/releases/tag/0.6.9)


### Bug Fixes

- Make fns work with non Copy ids (#476)

### Documentation

- Api reference and website examples update to a36a40a23b505c1eaa81b7ba8d5b2853e0e4bb86

### Features

- Void transaction  (#471)

### Miscellaneous Tasks

- Sqlx-prepare
- Cleanup leftover docker host refs (#474)

# [cala release v0.6.8](https://github.com/GaloyMoney/cala/releases/tag/0.6.8)


### Bug Fixes

- Early return for empty vectors in 'create_all_in_op' (#470)
- Early return for empty vectors in 'create_all_in_op'

### Documentation

- Api reference and website examples update to 43669726d308e9ad3b23562990bdcc27c7e8c464

### Miscellaneous Tasks

- Bump the all-dependencies group across 1 directory with 4 updates (#468)
- Bump flake

# [cala release v0.6.7](https://github.com/GaloyMoney/cala/releases/tag/0.6.7)


### Bug Fixes

- Attribute name in EsEntity (#466)

### Miscellaneous Tasks

- Bump the all-dependencies group in /website with 8 updates (#441)

# [cala release v0.6.6](https://github.com/GaloyMoney/cala/releases/tag/0.6.6)


### Documentation

- Api reference and website examples update to d582db2e1d0f9ed8189fc4797b6d0b690d3efc5e

### Miscellaneous Tasks

- Add entity_ty to es_query

# [cala release v0.6.5](https://github.com/GaloyMoney/cala/releases/tag/0.6.5)


### Bug Fixes

- Pass id_ty to es-query (#461)

### Documentation

- Api reference and website examples update to 5c7dcbd1305b4db2cb37e59de8dad97a05d1dbfd

### Miscellaneous Tasks

- Bump syn in the all-dependencies group (#460)
- Bump brace-expansion from 1.1.11 to 1.1.12 in /cala-nodejs (#443)

### Refactor

- Reorder id_ty in es_query

# [cala release v0.6.4](https://github.com/GaloyMoney/cala/releases/tag/0.6.4)


### Bug Fixes

- Regen schema

### Documentation

- Api reference and website examples update to 17060a3157b879a40676dc381cdba30e7b5a0010

### Miscellaneous Tasks

- Bump the all-dependencies group across 1 directory with 4 updates (#453)
- Add curl to flake (#456)
- Add currency field to entries query results (#450)

### Refactor

- Make setup-db in Makefile docker independent (#454)

# [cala release v0.6.3](https://github.com/GaloyMoney/cala/releases/tag/0.6.3)



# [cala release v0.5.3](https://github.com/GaloyMoney/cala/releases/tag/0.5.3)


### Bug Fixes

- Missing $crate::prelude in entity_id

### Documentation

- Api reference and website examples update to 1bec50ecd5ccd76dbfa2a1ac766dccdfcddc9b67

### Miscellaneous Tasks

- Add Account.entries to GQL layer (#424)
- Bump the all-dependencies group across 1 directory with 2 updates (#434)
- Bump cross-spawn from 7.0.3 to 7.0.6 in /cala-nodejs (#435)
- Bump path-to-regexp from 0.1.10 to 0.1.12 in /website (#430)
- Bump nanoid from 3.3.7 to 3.3.11 in /website (#431)
- Use workspace for sqlxprepare (#432)
- Bump the all-dependencies group in /website with 13 updates (#428)
- Update dependabot to group updates (#425)
- Bump tokio from 1.45.0 to 1.45.1 (#421)

# [cala release v0.5.2](https://github.com/GaloyMoney/cala/releases/tag/0.5.2)


### Bug Fixes

- Advisories (#418)

### Documentation

- Api reference and website examples update to fdd659a046f2e02d112c857466a5f4fb1d658a16

# [cala release v0.5.1](https://github.com/GaloyMoney/cala/releases/tag/0.5.1)


### Documentation

- Api reference and website examples update to 272f6ca4327d3a79d8b1221205aced0687894ed6

### Features

- Max_retries in es_entity::retry_on_concurrent_modification (#415)

### Miscellaneous Tasks

- Bump clap from 4.5.37 to 4.5.38 (#412)
- Add value to TransactionError::DuplicateXxx (#413)

# [cala release v0.5.0](https://github.com/GaloyMoney/cala/releases/tag/0.5.0)


### Documentation

- Api reference and website examples update to 87fe49893c205246c2557d3c7f1d742197f1ba62

### Miscellaneous Tasks

- [**breaking**] Remove in_range queries from balance (#411)

# [cala release v0.4.6](https://github.com/GaloyMoney/cala/releases/tag/0.4.6)


### Documentation

- Api reference and website examples update to ac4d7cb730b13f2cdd46dd8fe6f811494c1403b3

### Features

- Effective balances (#404)

### Miscellaneous Tasks

- Bump tokio from 1.44.2 to 1.45.0 (#406)
- Implement 'fetch_mappings_in_op' function (#408)
- Bump chrono from 0.4.40 to 0.4.41 (#402)
- Add JournalConfig (#403)
- Bump clap from 4.5.36 to 4.5.37 (#398)
- Bump syn from 2.0.100 to 2.0.101 (#400)
- Bump rand from 0.9.0 to 0.9.1 (#397)

### Refactor

- No pub(super) events (#401)
- Pub(super) is no longer needed on entity events

# [cala release v0.4.5](https://github.com/GaloyMoney/cala/releases/tag/0.4.5)


### Documentation

- Api reference and website examples update to 5e7b99346378083c5168bc7346fd11c9e7650145

### Features

- Add generics to PopulateNested (#396)

# [cala release v0.4.4](https://github.com/GaloyMoney/cala/releases/tag/0.4.4)


### Documentation

- Api reference and website examples update to 00179b430d9ae02a0b251c7280986cb1f91af239

### Miscellaneous Tasks

- Bump flake

# [cala release v0.4.3](https://github.com/GaloyMoney/cala/releases/tag/0.4.3)


### Documentation

- Api reference and website examples update to daf765865b6795a50c0fb2c2f35ed62b176b2b5a

### Miscellaneous Tasks

- Bump sqlx-ledger
- Bump anyhow from 1.0.97 to 1.0.98 (#391)
- Bump clap from 4.5.35 to 4.5.36 (#390)
- Bump proc-macro2 from 1.0.94 to 1.0.95 (#393)

# [cala release v0.4.2](https://github.com/GaloyMoney/cala/releases/tag/0.4.2)


### Documentation

- Api reference and website examples update to fd85eaadfe6ebe90d7f92e5235715b0c5ab65647

### Features

- Add TxTemplate::list (#389)

# [cala release v0.4.1](https://github.com/GaloyMoney/cala/releases/tag/0.4.1)



# [cala release v0.4.0](https://github.com/GaloyMoney/cala/releases/tag/0.4.0)


### Documentation

- Api reference and website examples update to 10ba39b038f5aba6cdad75d0c1244751717ba1e6

### Features

- Add ability to query transactions by template (#385)

### Refactor

- [**breaking**] Find_all_in_range fn (#383)

# [cala release v0.3.24](https://github.com/GaloyMoney/cala/releases/tag/0.3.24)


### Documentation

- Api reference and website examples update to 424d50f80e2093b5d782bb4ea4af106cc5509a8f

### Miscellaneous Tasks

- Add list_for_transaction_id to EntryRepo (#382)
- Bump tokio from 1.44.1 to 1.44.2 (#380)

# [cala release v0.3.23](https://github.com/GaloyMoney/cala/releases/tag/0.3.23)


### Bug Fixes

- 'list_members_by_created_at_in_op' naming (#377)

### Documentation

- Api reference and website examples update to ae1088e0f2ef2a256db7d5a59b75213f6b67072f

### Miscellaneous Tasks

- Bump rust_decimal_macros from 1.37.0 to 1.37.1 (#369)
- Bump napi-build from 2.1.5 to 2.1.6 (#371)
- Bump clap from 4.5.32 to 4.5.35 (#378)
- Derive Clone for PaginatedQueryArgs
- Bump flake (#372)

# [cala release v0.3.22](https://github.com/GaloyMoney/cala/releases/tag/0.3.22)


### Documentation

- Api reference and website examples update to 33d1eccf6d46baa185d71205b3e1a1ca2af98e45

### Features

- Introduce BTC and USD currency constants (#376)

### Miscellaneous Tasks

- Add 'get_persisted' to Nested (#375)
- Bump darling from 0.20.10 to 0.20.11 (#374)

### Refactor

- Fn visibility in AccountSet

# [cala release v0.3.21](https://github.com/GaloyMoney/cala/releases/tag/0.3.21)


### Documentation

- Api reference and website examples update to 32de413a01f749b53d7c037e2ca117c70f893de4

### Features

- Add 'expect' function to Idempotent (#368)
- Account set members by external id query  (#367)
- Add metadata to entries (#366)

# [cala release v0.3.20](https://github.com/GaloyMoney/cala/releases/tag/0.3.20)


### Bug Fixes

- Missing , in macro
- Idempotency_guard

### Documentation

- Api reference and website examples update to 49f33a156588e0fdf8a0eac38b7d06cc121d7790

### Miscellaneous Tasks

- Make idempotency_guard work in Result fns
- Bump napi from 2.16.16 to 2.16.17 (#360)
- Bump tokio from 1.42.0 to 1.44.1 (#361)
- Bump async-trait from 0.1.83 to 0.1.88 (#362)
- Bump thiserror from 1.0.69 to 2.0.12 (#363)
- Bump rust_decimal from 1.37.0 to 1.37.1 (#364)
- Bump sqlx
- Bump prost from 0.13.3 to 0.13.5 (#354)
- Bump async-graphql from 7.0.15 to 7.0.16 (#356)
- Bump pluralizer from 0.4.0 to 0.5.0 (#358)
- Bump serde_with from 3.11.0 to 3.12.0 (#355)

### Refactor

- Idempotent::AlreadyApplied -> Ignored
- Notify_cala_outbox_events fn (#359)

### Testing

- Fix clippy in balance.rs

# [cala release v0.3.19](https://github.com/GaloyMoney/cala/releases/tag/0.3.19)


### Bug Fixes

- Sqlx-prepare

### Documentation

- Api reference and website examples update to 8458c52364e2420f01400341d3e9f31fcfad9eb7

### Miscellaneous Tasks

- Add entries.list_for_journal_id

# [cala release v0.3.18](https://github.com/GaloyMoney/cala/releases/tag/0.3.18)


### Bug Fixes

- Add id to 'AccountSetMembersCursor' (#353)

### Documentation

- Api reference and website examples update to 1b4cb3d3403bf232108c90101e660eb5f1f47483

### Miscellaneous Tasks

- Bump rust_decimal from 1.36.0 to 1.37.0 (#348)
- Bump rust_decimal_macros from 1.36.0 to 1.37.0 (#349)
- Bump async-graphql-axum from 7.0.11 to 7.0.13 (#350)
- Bump quote from 1.0.37 to 1.0.40 (#351)
- Bump proc-macro2 from 1.0.92 to 1.0.94 (#352)
- Bump napi from 2.16.14 to 2.16.16 (#346)
- Bump async-graphql from 7.0.11 to 7.0.15 (#345)
- Bump rand to 0.9 (#310)
- Bump chrono from 0.4.38 to 0.4.40 (#344)
- Bump anyhow from 1.0.95 to 1.0.97 (#343)
- Bump serde_json from 1.0.138 to 1.0.140 (#339)
- Bump convert_case from 0.6.0 to 0.8.0 (#341)
- Bump serde from 1.0.217 to 1.0.219 (#342)
- Bump clap from 4.5.26 to 4.5.32 (#340)
- Bump uuid from 1.11.0 to 1.16.0 (#337)
- Bump micromatch from 4.0.5 to 4.0.8 in /cala-nodejs (#224)

# [cala release v0.3.17](https://github.com/GaloyMoney/cala/releases/tag/0.3.17)


### Documentation

- Api reference and website examples update to 8e5419b053a7c130418f8d27b91fca0b1337f9b1

### Miscellaneous Tasks

- Parse debit or credit from str (#338)
- Bump napi-derive from 2.16.12 to 2.16.13 (#299)
- Bump napi-build from 2.1.3 to 2.1.5 (#327)
- Bump syn from 2.0.90 to 2.0.100 (#332)

# [cala release v0.3.16](https://github.com/GaloyMoney/cala/releases/tag/0.3.16)


### Bug Fixes

- Clippy

### Documentation

- Api reference and website examples update to 4f7a93599cf8505978ceb6d50020a347534e3791

### Miscellaneous Tasks

- Bump ring from 0.17.8 to 0.17.13 (#331)

### Refactor

- Conditionally add GQL impl to core-types for primitives

# [cala release v0.3.15](https://github.com/GaloyMoney/cala/releases/tag/0.3.15)


### Bug Fixes

- Create account when batch creating account sets

### Documentation

- Api reference and website examples update to b1547bfb28d451979de5b7e42436736959b2f342

### Miscellaneous Tasks

- AccountSets.create_all

# [cala release v0.3.14](https://github.com/GaloyMoney/cala/releases/tag/0.3.14)


### Documentation

- Api reference and website examples update to 13201ed9f24fcabe2f2e24967dc602e927787992

### Miscellaneous Tasks

- Entries.list_for_account_set_id (#325)

# [cala release v0.3.13](https://github.com/GaloyMoney/cala/releases/tag/0.3.13)


### Documentation

- Api reference and website examples update to 01be799c32668b021f75f7057d5e643b40b29e22

### Miscellaneous Tasks

- Bump flake

### Testing

- Add generic Repo test for es-entity (#323)

# [cala release v0.3.12](https://github.com/GaloyMoney/cala/releases/tag/0.3.12)


### Bug Fixes

- Set-dev-version

### Documentation

- Api reference and website examples update to 40370f9503965f4ff395d4e70c30a21fccd34f49

### Features

- Add 'find_all_in_range' function (#320)

# [cala release v0.3.11](https://github.com/GaloyMoney/cala/releases/tag/0.3.11)



# [cala release v0.3.9](https://github.com/GaloyMoney/cala/releases/tag/0.3.9)


### Documentation

- Api reference and website examples update to ea1b75fa7fdc27f386a71143f97bd589a9c00dc9

### Miscellaneous Tasks

- Expose created_at fn for entry entity (#319)

# [cala release v0.3.8](https://github.com/GaloyMoney/cala/releases/tag/0.3.8)


### Bug Fixes

- Sqlx-prepare

# [cala release v0.3.7](https://github.com/GaloyMoney/cala/releases/tag/0.3.7)


### Documentation

- Api reference and website examples update to fcc1f6e474deb1b4a5ffe0379e6bf47d4db66a3f

# [cala release v0.3.6](https://github.com/GaloyMoney/cala/releases/tag/0.3.6)


### Documentation

- Api reference and website examples update to 62829b08802ef71e2f4127fda5ecc8146fb26bc2

### Features

- Add external_id to account set (#318)

### Miscellaneous Tasks

- Bump napi from 2.16.13 to 2.16.14 (#312)
- Bump serde from 1.0.215 to 1.0.217 (#311)

# [cala release v0.3.5](https://github.com/GaloyMoney/cala/releases/tag/0.3.5)


### Documentation

- Api reference and website examples update to 44afb1b1d7ae13a0f2e0a5673df7d0938ecbf791

### Features

- Add new 'list_for_name_in_op' function (#314)

# [cala release v0.3.4](https://github.com/GaloyMoney/cala/releases/tag/0.3.4)


### Documentation

- Api reference and website examples update to e6f01654bccca6f97298fa70f6b8a8107ac5be2d

### Features

- Add 'in_tx' for list fns in es-entity (#313)

# [cala release v0.3.3](https://github.com/GaloyMoney/cala/releases/tag/0.3.3)


### Bug Fixes

- Update latest_balances instead of new_balance (#294)

### Documentation

- Api reference and website examples update to 91bfafe61feaad13b36c2e92c489393ebaceb219

### Features

- Add AccountSet list_for_name query (#305)
- Wait until realtime fn (#304)

### Miscellaneous Tasks

- Bump serde_json from 1.0.133 to 1.0.138 (#307)
- Member already added error (#306)
- Attempt to fix concurrent update of balances
- More precise error when limit already added to control (#297)
- More specific error on duplicate limit id (#296)
- Bump tokio-stream from 0.1.16 to 0.1.17 (#280)
- Bump anyhow from 1.0.93 to 1.0.95 (#284)
- Bump clap from 4.5.21 to 4.5.26 (#293)
- More specific error on duplicate control id (#295)
- Bump flake
- Remove begin_sub_operation
- Begin_sub_operation
- Run sqlx prepare (#283)
- Journals().find_by_code
- Add CodeAlreadyExists to JournalError
- Add code to journal
- Accounts.create_all
- Bump syn from 2.0.89 to 2.0.90 (#274)
- Bump tracing-subscriber from 0.3.18 to 0.3.19 (#275)
- Bump tokio from 1.41.1 to 1.42.0 (#276)
- Remove core entities and technical specifications section (#278)
- Add missing indexes on transaction (#277)
- Tx_template DuplicateCode error

### Refactor

- Find for update in velocity (#309)
- Better balance locking (#308)

# [cala release v0.3.2](https://github.com/GaloyMoney/cala/releases/tag/0.3.2)


### Documentation

- Improve README (#273)
- Api reference and website examples update to ef6f8659c113a9f52e1a03ce58de046423e2f7f6

# [cala release v0.3.1](https://github.com/GaloyMoney/cala/releases/tag/0.3.1)


### Documentation

- Api reference and website examples update to de6fc30b7065d690e2cd10b7c7092b0eb2aa88cd

### Refactor

- Return n_events from update

# [cala release v0.3.0](https://github.com/GaloyMoney/cala/releases/tag/0.3.0)


### Bug Fixes

- Cargo.toml for lib

### Documentation

- Api reference and website examples update to 7986721300633d1abdbff87dec81b20202b866fb

### Miscellaneous Tasks

- Remove README ref from sim-time
- || true for cala-tracing publish (tmp)
- Clippy fix
- Bump flake
- Bump serde from 1.0.210 to 1.0.215 (#266)
- Bump axum from 0.7.7 to 0.7.9 (#267)
- Bump uuid from 1.10.0 to 1.11.0 (#255)
- Bump clap from 4.5.18 to 4.5.21 (#263)
- Bump serde_json from 1.0.128 to 1.0.133 (#264)
- Bump flake

### Refactor

- [**breaking**] Es-entity list_by default = false
- Use es-entity (#272)

# [cala release v0.2.46](https://github.com/GaloyMoney/cala/releases/tag/0.2.46)


### Documentation

- Api reference and website examples update to 9490239af64d3b088d5e6e5e62e15e2aad81d14f

### Miscellaneous Tasks

- Remove dbg

# [cala release v0.2.45](https://github.com/GaloyMoney/cala/releases/tag/0.2.45)


### Documentation

- Add enterprise solutions page (#211)
- Api reference and website examples update to a443b6720a24e2fba588331572966efc45cd2b5d

### Features

- VelocityControlCreate mutation (#250)
- VelocityLimitCreate mutation (#248)

### Miscellaneous Tasks

- VelocityLimit and velocityControl queries (#256)
- VelocityControlAttach mutation (#253)
- VelocityControlLimitAdd (#252)
- Add timestamps to Transaction (#247)
- Bump axum from 0.7.6 to 0.7.7 (#236)
- Bump napi-derive from 2.16.11 to 2.16.12 (#233)
- Bump tonic-build from 0.12.2 to 0.12.3 (#234)
- Bump tonic-health from 0.12.2 to 0.12.3 (#235)
- Bump serde_with from 3.9.0 to 3.11.0 (#239)
- Add support for member fns (#242)
- Velocity enforcement (#237)
- Add velocity control and attach to account boilerplate (#225)

### Refactor

- No need for Runner.run to be mut
- Remove events from Job entity (#249)

### Testing

- Velocity limits (#254)
- Add unit tests for enforce (#241)
- Velocity (#240)

# [cala release v0.2.44](https://github.com/GaloyMoney/cala/releases/tag/0.2.44)


### Documentation

- Improve descriptions, make them not clickable (#210)
- Api reference and website examples update to a6eb83982dea4402fb2206d55e12080d85cd79c1

### Miscellaneous Tasks

- Bump flake
- Bump micromatch from 4.0.5 to 4.0.8 in /website (#215)
- Bump body-parser and express in /website (#219)
- Bump webpack from 5.91.0 to 5.94.0 in /website (#220)
- Bump anyhow from 1.0.86 to 1.0.89 (#222)
- Bump axum from 0.7.5 to 0.7.6 (#221)
- Bump tonic-health from 0.12.1 to 0.12.2 (#209)
- Bump napi from 2.16.9 to 2.16.11 (#213)
- Bump clap from 4.5.16 to 4.5.18 (#212)
- Bump derive_builder from 0.20.0 to 0.20.1 (#207)
- Bump prost from 0.13.1 to 0.13.3 (#214)
- Bump tonic-build from 0.12.1 to 0.12.2 (#205)
- Bump tokio-stream from 0.1.15 to 0.1.16 (#206)

# [cala release v0.2.43](https://github.com/GaloyMoney/cala/releases/tag/0.2.43)


### Documentation

- Api reference and website examples update to ac43986a3fe0b96202ebd8e6fc00429622d25b46

### Miscellaneous Tasks

- Bump serde_json from 1.0.124 to 1.0.128 (#204)
- Bump rust_decimal_macros from 1.35.0 to 1.36.0 (#198)
- Bump sqlx from 0.8.0 to 0.8.2 (#203)
- Bump rust_decimal from 1.35.0 to 1.36.0 (#199)
- Bump tokio from 1.39.2 to 1.40.0 (#202)
- Bump pg images

# [cala release v0.2.42](https://github.com/GaloyMoney/cala/releases/tag/0.2.42)


### Documentation

- Api reference and website examples update to a783e53d38e1152ac8b1b4fafcd465739ff4e671

### Miscellaneous Tasks

- Add version

# [cala release v0.2.41](https://github.com/GaloyMoney/cala/releases/tag/0.2.41)


### Bug Fixes

- Connect traces in graphql server

### Documentation

- Api reference and website examples update to 963f601fe48db7b01b46db7fa42389c0b62cc74a

# [cala release v0.2.40](https://github.com/GaloyMoney/cala/releases/tag/0.2.40)


### Documentation

- Api reference and website examples update to f75aa46579c54f19bdb4258b38fd2e380f2db0d2

### Miscellaneous Tasks

- Add audit ignore
- Bump sqlx

# [cala release v0.2.39](https://github.com/GaloyMoney/cala/releases/tag/0.2.39)


### Documentation

- Api reference and website examples update to 583540e1187a541ee0dc21b04931fcfccd17c4c8

### Miscellaneous Tasks

- Add missing entry.version

# [cala release v0.2.38](https://github.com/GaloyMoney/cala/releases/tag/0.2.38)


### Documentation

- Api reference and website examples update to d034d4be06042201ced335c5f4879143479ad256

### Miscellaneous Tasks

- Bump grpc deps (#193)
- Bump flake

# [cala release v0.2.37](https://github.com/GaloyMoney/cala/releases/tag/0.2.37)


### Documentation

- Api reference and website examples update to 23cdd46976f0ab68ce8b5959642bb7c914c4ef96

### Miscellaneous Tasks

- Bump serde from 1.0.207 to 1.0.208 (#191)
- Bump napi from 2.16.8 to 2.16.9 (#192)
- Bump clap from 4.5.15 to 4.5.16 (#190)
- Bump async-graphql-axum from 7.0.6 to 7.0.7 (#186)
- Bump tokio from 1.38.0 to 1.39.2 (#179)
- Bump napi-derive from 2.16.10 to 2.16.11 (#185)
- Bump serde from 1.0.204 to 1.0.207 (#187)

# [cala release v0.2.36](https://github.com/GaloyMoney/cala/releases/tag/0.2.36)


### Bug Fixes

- Balance in range query (#189)

### Documentation

- Api reference and website examples update to 335fcd2919c6f3e1364de5bab436a1c403f927ec

# [cala release v0.2.35](https://github.com/GaloyMoney/cala/releases/tag/0.2.35)


### Bug Fixes

- Correct ordering for find_in_range

### Documentation

- Api reference and website examples update to 0aa97bf767af5449ad2424def293d21fc6e8f981

# [cala release v0.2.34](https://github.com/GaloyMoney/cala/releases/tag/0.2.34)


### Bug Fixes

- Use advisory lock for account set manipulations

### Documentation

- Api reference and website examples update to b4de8e982bd4318d230c32bb3f99e9b3810b4db8

# [cala release v0.2.33](https://github.com/GaloyMoney/cala/releases/tag/0.2.33)


### Bug Fixes

- Sqlx-prepare
- More exclusive lock on adding / removing account set members

### Documentation

- Api reference and website examples update to cee2e78c72059c1cc470e75e1c5cf9a04dfa1f2c

# [cala release v0.2.32](https://github.com/GaloyMoney/cala/releases/tag/0.2.32)


### Bug Fixes

- Lock account set table when adding / removing members (#188)

### Documentation

- Api reference and website examples update to 6ebb671473ba843b736ee7e4213044e9a007ea1c

### Miscellaneous Tasks

- Bump thiserror from 1.0.62 to 1.0.63 (#180)
- Bump clap from 4.5.9 to 4.5.15 (#183)
- Bump serde_json from 1.0.120 to 1.0.124 (#184)
- Bump async-trait from 0.1.80 to 0.1.81 (#159)

# [cala release v0.2.31](https://github.com/GaloyMoney/cala/releases/tag/0.2.31)


### Bug Fixes

- Pull and rebase before push to avoid bot conflicts (#175)

### Documentation

- Api reference and website examples update: 2054b0d14362aa298f90a107c173a1613271cd3a

### Miscellaneous Tasks

- Bump serde_with from 3.8.3 to 3.9.0 (#169)
- Bump napi-derive from 2.16.8 to 2.16.10 (#174)
- Bump async-graphql from 7.0.6 to 7.0.7 (#170)

# [cala release v0.2.30](https://github.com/GaloyMoney/cala/releases/tag/0.2.30)


### Documentation

- Api reference update: 6004a972cd7bdcc70a56590a02dbb18e0dfccf32

### Miscellaneous Tasks

- Add balance_in_range to account_set (#173)
- Add website-demo.bats to generate json variables and response files (#172)
- Bump clap from 4.5.8 to 4.5.9 (#164)
- Bump thiserror from 1.0.61 to 1.0.62 (#166)
- Bump rust_decimal_macros from 1.34.2 to 1.35.0 (#167)

# [cala release v0.2.29](https://github.com/GaloyMoney/cala/releases/tag/0.2.29)


### Bug Fixes

- Members query in account set  (#165)

### Documentation

- Add account set page to website demo (#163)
- Add plugin-google-gtag and analytics to website (#156)
- Api reference update: d8ab630af31ef4b468a5ab46d639af8df5f57356

### Miscellaneous Tasks

- Bump serde_with from 3.8.2 to 3.8.3 (#158)
- Bump serde from 1.0.203 to 1.0.204 (#160)
- Bump uuid from 1.9.1 to 1.10.0 (#162)

# [cala release v0.2.28](https://github.com/GaloyMoney/cala/releases/tag/0.2.28)


### Documentation

- Api reference update: 946c0d765dd038b56ccd982ecfb2ab6f12d954bd

### Miscellaneous Tasks

- Balance in range query (#157)

# [cala release v0.2.27](https://github.com/GaloyMoney/cala/releases/tag/0.2.27)


### Documentation

- Open api reference search hits in external window (#152)
- Api reference update: 3dba1f351bc3198af7cb6b6cca4b6dd36e6a8111

### Miscellaneous Tasks

- Bump serde_json from 1.0.118 to 1.0.120 (#153)
- Bump napi-derive from 2.16.6 to 2.16.8 (#154)
- Balance_as_of (#155)
- Bump napi from 2.16.7 to 2.16.8 (#141)

### Refactor

- As-of -> since

# [cala release v0.2.26](https://github.com/GaloyMoney/cala/releases/tag/0.2.26)


### Documentation

- Api reference update: 51d11f1cf72f00a76aa40c6148fd248b450343bb

### Miscellaneous Tasks

- Bump clap from 4.5.7 to 4.5.8 (#150)
- Bump serde_with from 3.8.1 to 3.8.2 (#151)
- Drop Input suffix from Velocity Limit (#147)

# [cala release v0.2.25](https://github.com/GaloyMoney/cala/releases/tag/0.2.25)


### Documentation

- Api reference update: 43591ec3fbc9a3a71dd51f1ddcabf34a97c023a4

### Miscellaneous Tasks

- Available balance (#146)

# [cala release v0.2.24](https://github.com/GaloyMoney/cala/releases/tag/0.2.24)



# [cala release v0.2.21](https://github.com/GaloyMoney/cala/releases/tag/0.2.21)


### Bug Fixes

- Filter transitive for list_children

### Documentation

- Add search to website (#138)
- Api reference update: 2c66d873a79f00b71395474f3fda7bfea604bb03

# [cala release v0.2.20](https://github.com/GaloyMoney/cala/releases/tag/0.2.20)


### Bug Fixes

- Deploy api reference action (#135)

### Documentation

- Api reference update: 22ce6f31ffa3b8fbe2dc96c3fff59196e5dfd678
- Add accounting intro pages (#125)

### Miscellaneous Tasks

- PostTransaction -> transactionPost (#137)
- Nix in actions, deploy website after API reference update (#136)
- Bump serde_json from 1.0.117 to 1.0.118 (#131)
- Bump uuid from 1.9.0 to 1.9.1 (#130)
- Bump napi from 2.16.6 to 2.16.7 (#133)
- Bump napi-derive from 2.16.5 to 2.16.6 (#132)

# [cala release v0.2.19](https://github.com/GaloyMoney/cala/releases/tag/0.2.19)


### Documentation

- Add graphql api demo explanations (#121)
- Clickable landing page items (#120)
- Generate and update API reference for cala.sh (#117)

### Miscellaneous Tasks

- Account set member (#122)
- Bump lazy_static from 1.4.0 to 1.5.0 (#123)
- Bump uuid from 1.8.0 to 1.9.0 (#124)

# [cala release v0.2.18](https://github.com/GaloyMoney/cala/releases/tag/0.2.18)


### Miscellaneous Tasks

- Persist_at_in_tx under imported flag (#119)

# [cala release v0.2.17](https://github.com/GaloyMoney/cala/releases/tag/0.2.17)


### Features

- Add sets query on account sets (#116)

### Miscellaneous Tasks

- Don't skip member from tracing (#118)

# [cala release v0.2.16](https://github.com/GaloyMoney/cala/releases/tag/0.2.16)


### Features

- Impl accountSetUpdate (#111)
- Impl journalUpdate (#110)
- AccountUpdate mutation (#106)

### Miscellaneous Tasks

- Read balance in op (#115)
- Impl removeFromAccountSet (#114)
- Add tx_template_id to transactions table (#113)
- Move account set update test to account_set.rs (#112)
- Bump async-graphql-axum from 7.0.5 to 7.0.6 (#102)
- Bump clap from 4.5.6 to 4.5.7 (#101)
- Bump cached from 0.51.3 to 0.51.4 (#109)

# [cala release v0.2.15](https://github.com/GaloyMoney/cala/releases/tag/0.2.15)


### Miscellaneous Tasks

- Expose pool / sqlx

# [cala release v0.2.14](https://github.com/GaloyMoney/cala/releases/tag/0.2.14)


### Bug Fixes

- Prohibit job intiializers overwriting
- Filter by data_source_id (#105)

### Miscellaneous Tasks

- Impl find_by_id for transaction (#108)
- Expose find fn for jobs (#107)

# [cala release v0.2.13](https://github.com/GaloyMoney/cala/releases/tag/0.2.13)


### Bug Fixes

- Update some queries
- Order by e.sequence (#104)

# [cala release v0.2.12](https://github.com/GaloyMoney/cala/releases/tag/0.2.12)


### Features

- Add accountByCode query (#103)

# [cala release v0.2.11](https://github.com/GaloyMoney/cala/releases/tag/0.2.11)



# [cala release v0.2.10](https://github.com/GaloyMoney/cala/releases/tag/0.2.10)


### Bug Fixes

- Lint

### Miscellaneous Tasks

- Setter(into) on journal status
- Make more graphql mods public
- Make JobRunner.run accept &mut self
- From<Decimal> for rust_decimal::DEcimal

# [cala release v0.2.9](https://github.com/GaloyMoney/cala/releases/tag/0.2.9)


### Refactor

- Pass owned Job in init

# [cala release v0.2.8](https://github.com/GaloyMoney/cala/releases/tag/0.2.8)


### Miscellaneous Tasks

- Add JobEvent::Updated

# [cala release v0.2.7](https://github.com/GaloyMoney/cala/releases/tag/0.2.7)


### Features

- Add sets query to account (#100)

### Refactor

- Inject job_id (#99)

# [cala release v0.2.6](https://github.com/GaloyMoney/cala/releases/tag/0.2.6)


### Refactor

- Pass IntegrationId

# [cala release v0.2.5](https://github.com/GaloyMoney/cala/releases/tag/0.2.5)


### Bug Fixes

- Doc tests
- Sqlx-prepare

### Miscellaneous Tasks

- Bump async-graphql from 7.0.5 to 7.0.6 (#97)
- Bump clap from 4.5.4 to 4.5.6 (#96)
- Add DuplicateKey error
- Explicit DuplicateKey error for templates

### Refactor

- No Option<TxParams> for post_transaction
- Include JobCompletion
- Job.config -> data
- Integration config -> data
- Expose graphql account

# [cala release v0.2.4](https://github.com/GaloyMoney/cala/releases/tag/0.2.4)


### Bug Fixes

- Lookup in same tx
- Commit AtomicOperation without events
- Check-code

### Documentation

- Docs.rs landing page (#95)

### Features

- Add integrations

### Refactor

- Add Jobs service layer

# [cala release v0.2.3](https://github.com/GaloyMoney/cala/releases/tag/0.2.3)


### Bug Fixes

- Sql syntax
- Expose ledger / types from correct module

### Miscellaneous Tasks

- Expose as_bytes

# [cala release v0.2.2](https://github.com/GaloyMoney/cala/releases/tag/0.2.2)


### Miscellaneous Tasks

- Expose more stuff
- Expose ledger in CalaApp

# [cala release v0.2.1](https://github.com/GaloyMoney/cala/releases/tag/0.2.1)


### Miscellaneous Tasks

- Enable QueryExtension

# [cala release v0.2.0](https://github.com/GaloyMoney/cala/releases/tag/0.2.0)


### Bug Fixes

- Remove feature in AccountSetRepo

# [cala release v0.1.11](https://github.com/GaloyMoney/cala/releases/tag/0.1.11)


### Documentation

- Fix edit this page links (#91)

### Testing

- Add doc test to ci (#94)

# [cala release v0.1.10](https://github.com/GaloyMoney/cala/releases/tag/0.1.10)


### Miscellaneous Tasks

- Make poll_jobs a trace

# [cala release v0.1.9](https://github.com/GaloyMoney/cala/releases/tag/0.1.9)


### Bug Fixes

- Multiple entries for same account (#92)

# [cala release v0.1.8](https://github.com/GaloyMoney/cala/releases/tag/0.1.8)


### Documentation

- Add website with graphql api demo (#55)

### Miscellaneous Tasks

- Rename encumbered -> encumbrance (#90)
- Remove CALA_SERVER_ID env variable (#89)

# [cala release v0.1.7](https://github.com/GaloyMoney/cala/releases/tag/0.1.7)


### Bug Fixes

- Cala name (#87)

### Features

- Update set balances (#86)
- Transitive account sets (#83)

### Miscellaneous Tasks

- Bump tokio from 1.37.0 to 1.38.0 (#85)
- Use default config when no config file specified (#82)
- Bump serde from 1.0.202 to 1.0.203 (#75)

### Refactor

- Prohibit multi-set inclusion (#84)

# [cala release v0.1.6](https://github.com/GaloyMoney/cala/releases/tag/0.1.6)


### Bug Fixes

- Clippy

### Refactor

- Use async_graphql::parser::types prefix
- Simplify match
- Handle docs with multiple ops

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
