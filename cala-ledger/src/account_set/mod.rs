mod cursor;
mod entity;
pub mod error;
mod repo;

#[cfg(feature = "import")]
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use std::collections::HashMap;
use tracing::instrument;

#[cfg(feature = "import")]
use crate::primitives::DataSourceId;
use crate::{
    account::*,
    atomic_operation::*,
    balance::*,
    entry::*,
    outbox::*,
    primitives::{DataSource, DebitOrCredit, JournalId, Layer},
    query::*,
};

pub use cursor::*;
pub use entity::*;
use error::*;
use repo::*;

#[allow(dead_code)]
const UNASSIGNED_TRANSACTION_ID: uuid::Uuid = uuid::Uuid::nil();

#[derive(Clone)]
pub struct AccountSets {
    repo: AccountSetRepo,
    accounts: Accounts,
    entries: Entries,
    balances: Balances,
    outbox: Outbox,
    pool: PgPool,
}

impl AccountSets {
    pub(crate) fn new(
        pool: &PgPool,
        outbox: Outbox,
        accounts: &Accounts,
        entries: &Entries,
        balances: &Balances,
    ) -> Self {
        Self {
            repo: AccountSetRepo::new(pool),
            outbox,
            accounts: accounts.clone(),
            entries: entries.clone(),
            balances: balances.clone(),
            pool: pool.clone(),
        }
    }
    #[instrument(name = "cala_ledger.account_sets.create", skip(self))]
    pub async fn create(
        &self,
        new_account_set: NewAccountSet,
    ) -> Result<AccountSet, AccountSetError> {
        let mut op = AtomicOperation::init(&self.pool, &self.outbox).await?;
        let account_set = self.create_in_op(&mut op, new_account_set).await?;
        op.commit().await?;
        Ok(account_set)
    }

    #[instrument(name = "cala_ledger.account_sets.create", skip(self, op))]
    pub async fn create_in_op(
        &self,
        op: &mut AtomicOperation<'_>,
        new_account_set: NewAccountSet,
    ) -> Result<AccountSet, AccountSetError> {
        let new_account = NewAccount::builder()
            .id(uuid::Uuid::from(new_account_set.id))
            .name(String::new())
            .code(new_account_set.id.to_string())
            .normal_balance_type(new_account_set.normal_balance_type)
            .is_account_set(true)
            .build()
            .expect("Failed to build account");
        self.accounts.create_in_op(op, new_account).await?;
        let account_set = self.repo.create_in_tx(op.tx(), new_account_set).await?;
        op.accumulate(account_set.events.last_persisted());
        Ok(account_set)
    }

    #[instrument(name = "cala_ledger.account_sets.persist", skip(self, account_set))]
    pub async fn persist(&self, account_set: &mut AccountSet) -> Result<(), AccountSetError> {
        let mut op = AtomicOperation::init(&self.pool, &self.outbox).await?;
        self.persist_in_op(&mut op, account_set).await?;
        op.commit().await?;
        Ok(())
    }

    #[instrument(
        name = "cala_ledger.account_sets.persist_in_op",
        skip(self, op, account_set)
    )]
    pub async fn persist_in_op(
        &self,
        op: &mut AtomicOperation<'_>,
        account_set: &mut AccountSet,
    ) -> Result<(), AccountSetError> {
        self.repo.persist_in_tx(op.tx(), account_set).await?;
        op.accumulate(account_set.events.last_persisted());
        Ok(())
    }

    pub async fn add_member(
        &self,
        account_set_id: AccountSetId,
        member: impl Into<AccountSetMember>,
    ) -> Result<AccountSet, AccountSetError> {
        let mut op = AtomicOperation::init(&self.pool, &self.outbox).await?;
        let account_set = self
            .add_member_in_op(&mut op, account_set_id, member)
            .await?;
        op.commit().await?;
        Ok(account_set)
    }

    pub async fn add_member_in_op(
        &self,
        op: &mut AtomicOperation<'_>,
        account_set_id: AccountSetId,
        member: impl Into<AccountSetMember>,
    ) -> Result<AccountSet, AccountSetError> {
        let member = member.into();
        let (time, parents, account_set, member_id) = match member {
            AccountSetMember::Account(id) => {
                let set = self.repo.find_in_tx(op.tx(), account_set_id).await?;
                let (time, parents) = self
                    .repo
                    .add_member_account_and_return_parents(op.tx(), account_set_id, id)
                    .await?;
                (time, parents, set, id)
            }
            AccountSetMember::AccountSet(id) => {
                let mut accounts = self
                    .repo
                    .find_all_in_tx::<AccountSet>(op.tx(), &[account_set_id, id])
                    .await?;
                let target = accounts
                    .remove(&account_set_id)
                    .ok_or(AccountSetError::CouldNotFindById(account_set_id))?;
                let member = accounts
                    .remove(&id)
                    .ok_or(AccountSetError::CouldNotFindById(id))?;

                if target.values().journal_id != member.values().journal_id {
                    return Err(AccountSetError::JournalIdMismatch);
                }

                let (time, parents) = self
                    .repo
                    .add_member_set_and_return_parents(op.tx(), account_set_id, id)
                    .await?;
                (time, parents, target, AccountId::from(id))
            }
        };

        op.accumulate(std::iter::once(
            OutboxEventPayload::AccountSetMemberCreated {
                source: DataSource::Local,
                account_set_id,
                member,
            },
        ));

        let balances = self
            .balances
            .find_balances_for_update(op.tx(), account_set.values().journal_id, member_id)
            .await?;

        let target_account_id = AccountId::from(&account_set.id());
        let mut entries = Vec::new();
        for balance in balances.into_values() {
            entries_for_add_balance(&mut entries, target_account_id, balance);
        }

        if entries.is_empty() {
            return Ok(account_set);
        }
        let entries = self.entries.create_all_in_op(op, entries).await?;
        let mappings = std::iter::once((target_account_id, parents)).collect();
        self.balances
            .update_balances_in_op(op, time, account_set.values().journal_id, entries, mappings)
            .await?;

        Ok(account_set)
    }

    pub async fn remove_member(
        &self,
        account_set_id: AccountSetId,
        member: impl Into<AccountSetMember>,
    ) -> Result<AccountSet, AccountSetError> {
        let mut op = AtomicOperation::init(&self.pool, &self.outbox).await?;
        let account_set = self
            .remove_member_in_op(&mut op, account_set_id, member)
            .await?;
        op.commit().await?;
        Ok(account_set)
    }

    pub async fn remove_member_in_op(
        &self,
        op: &mut AtomicOperation<'_>,
        account_set_id: AccountSetId,
        member: impl Into<AccountSetMember>,
    ) -> Result<AccountSet, AccountSetError> {
        let member = member.into();
        let (time, parents, account_set, member_id) = match member {
            AccountSetMember::Account(id) => {
                let set = self.repo.find_in_tx(op.tx(), account_set_id).await?;
                let (time, parents) = self
                    .repo
                    .remove_member_account_and_return_parents(op.tx(), account_set_id, id)
                    .await?;
                (time, parents, set, id)
            }
            AccountSetMember::AccountSet(id) => {
                let mut accounts = self
                    .repo
                    .find_all_in_tx::<AccountSet>(op.tx(), &[account_set_id, id])
                    .await?;
                let target = accounts
                    .remove(&account_set_id)
                    .ok_or(AccountSetError::CouldNotFindById(account_set_id))?;
                let member = accounts
                    .remove(&id)
                    .ok_or(AccountSetError::CouldNotFindById(id))?;

                if target.values().journal_id != member.values().journal_id {
                    return Err(AccountSetError::JournalIdMismatch);
                }

                let (time, parents) = self
                    .repo
                    .remove_member_set_and_return_parents(op.tx(), account_set_id, id)
                    .await?;
                (time, parents, target, AccountId::from(id))
            }
        };

        op.accumulate(std::iter::once(
            OutboxEventPayload::AccountSetMemberRemoved {
                source: DataSource::Local,
                account_set_id,
                member,
            },
        ));

        let balances = self
            .balances
            .find_balances_for_update(op.tx(), account_set.values().journal_id, member_id)
            .await?;

        let target_account_id = AccountId::from(&account_set.id());
        let mut entries = Vec::new();
        for balance in balances.into_values() {
            entries_for_remove_balance(&mut entries, target_account_id, balance);
        }

        if entries.is_empty() {
            return Ok(account_set);
        }
        let entries = self.entries.create_all_in_op(op, entries).await?;
        let mappings = std::iter::once((target_account_id, parents)).collect();
        self.balances
            .update_balances_in_op(op, time, account_set.values().journal_id, entries, mappings)
            .await?;

        Ok(account_set)
    }

    #[instrument(name = "cala_ledger.account_sets.find_all", skip(self), err)]
    pub async fn find_all<T: From<AccountSet>>(
        &self,
        account_set_ids: &[AccountSetId],
    ) -> Result<HashMap<AccountSetId, T>, AccountSetError> {
        self.repo.find_all(account_set_ids).await
    }

    #[instrument(name = "cala_ledger.account_sets.find", skip(self), err)]
    pub async fn find(&self, account_set_id: AccountSetId) -> Result<AccountSet, AccountSetError> {
        self.repo.find(account_set_id).await
    }

    #[instrument(
        name = "cala_ledger.account_sets.find_where_account_is_member",
        skip(self),
        err
    )]
    pub async fn find_where_account_is_member(
        &self,
        account_id: AccountId,
        query: PaginatedQueryArgs<AccountSetByNameCursor>,
    ) -> Result<PaginatedQueryRet<AccountSet, AccountSetByNameCursor>, AccountSetError> {
        self.repo
            .find_where_account_is_member(account_id, query)
            .await
    }

    #[instrument(
        name = "cala_ledger.account_sets.find_where_account_set_is_member",
        skip(self),
        err
    )]
    pub async fn find_where_account_set_is_member(
        &self,
        account_set_id: AccountSetId,
        query: PaginatedQueryArgs<AccountSetByNameCursor>,
    ) -> Result<PaginatedQueryRet<AccountSet, AccountSetByNameCursor>, AccountSetError> {
        self.repo
            .find_where_account_set_is_member(account_set_id, query)
            .await
    }

    #[instrument(
        name = "cala_ledger.account_sets.find_where_account_set_is_member_in_op",
        skip(self, op),
        err
    )]
    pub async fn find_where_account_set_is_member_in_op(
        &self,
        op: &mut AtomicOperation<'_>,
        account_set_id: AccountSetId,
        query: PaginatedQueryArgs<AccountSetByNameCursor>,
    ) -> Result<PaginatedQueryRet<AccountSet, AccountSetByNameCursor>, AccountSetError> {
        self.repo
            .find_where_account_set_is_member_in_tx(op.tx(), account_set_id, query)
            .await
    }

    pub(crate) async fn fetch_mappings(
        &self,
        journal_id: JournalId,
        account_ids: &[AccountId],
    ) -> Result<HashMap<AccountId, Vec<AccountSetId>>, AccountSetError> {
        self.repo.fetch_mappings(journal_id, account_ids).await
    }

    #[cfg(feature = "import")]
    pub async fn sync_account_set_creation(
        &self,
        mut db: sqlx::Transaction<'_, sqlx::Postgres>,
        recorded_at: DateTime<Utc>,
        origin: DataSourceId,
        values: AccountSetValues,
    ) -> Result<(), AccountSetError> {
        let mut account_set = AccountSet::import(origin, values);
        self.repo
            .import(&mut db, recorded_at, origin, &mut account_set)
            .await?;
        self.outbox
            .persist_events_at(db, account_set.events.last_persisted(), recorded_at)
            .await?;
        Ok(())
    }

    #[cfg(feature = "import")]
    pub async fn sync_account_set_update(
        &self,
        mut db: sqlx::Transaction<'_, sqlx::Postgres>,
        recorded_at: DateTime<Utc>,
        origin: DataSourceId,
        values: AccountSetValues,
        fields: Vec<String>,
    ) -> Result<(), AccountSetError> {
        let mut account_set = self.repo.find_imported(values.id, origin).await?;
        account_set.update((values, fields));
        self.repo
            .persist_at_in_tx(&mut db, recorded_at, origin, &mut account_set)
            .await?;
        self.outbox
            .persist_events_at(db, account_set.events.last_persisted(), recorded_at)
            .await?;
        Ok(())
    }

    #[cfg(feature = "import")]
    pub async fn sync_account_set_member_creation(
        &self,
        mut db: sqlx::Transaction<'_, sqlx::Postgres>,
        recorded_at: DateTime<Utc>,
        origin: DataSourceId,
        account_set_id: AccountSetId,
        member: AccountSetMember,
    ) -> Result<(), AccountSetError> {
        match member {
            AccountSetMember::Account(account_id) => {
                self.repo
                    .import_member_account(&mut db, recorded_at, origin, account_set_id, account_id)
                    .await?;
            }
            AccountSetMember::AccountSet(account_set_id) => {
                self.repo
                    .import_member_set(&mut db, recorded_at, origin, account_set_id, account_set_id)
                    .await?;
            }
        }
        self.outbox
            .persist_events_at(
                db,
                std::iter::once(OutboxEventPayload::AccountSetMemberCreated {
                    source: DataSource::Remote { id: origin },
                    account_set_id,
                    member,
                }),
                recorded_at,
            )
            .await?;
        Ok(())
    }

    #[cfg(feature = "import")]
    pub async fn sync_account_set_member_removal(
        &self,
        mut db: sqlx::Transaction<'_, sqlx::Postgres>,
        recorded_at: DateTime<Utc>,
        origin: DataSourceId,
        account_set_id: AccountSetId,
        member: AccountSetMember,
    ) -> Result<(), AccountSetError> {
        match member {
            AccountSetMember::Account(account_id) => {
                self.repo
                    .import_remove_member_account(&mut db, origin, account_set_id, account_id)
                    .await?;
            }
            AccountSetMember::AccountSet(account_set_id) => {
                self.repo
                    .import_remove_member_set(&mut db, origin, account_set_id, account_set_id)
                    .await?;
            }
        }
        self.outbox
            .persist_events_at(
                db,
                std::iter::once(OutboxEventPayload::AccountSetMemberRemoved {
                    source: DataSource::Remote { id: origin },
                    account_set_id,
                    member,
                }),
                recorded_at,
            )
            .await?;
        Ok(())
    }
}

fn entries_for_add_balance(
    entries: &mut Vec<NewEntry>,
    target_account_id: AccountId,
    balance: BalanceSnapshot,
) {
    use rust_decimal::Decimal;

    if balance.settled_cr_balance != Decimal::ZERO {
        let entry = NewEntry::builder()
            .id(EntryId::new())
            .journal_id(balance.journal_id)
            .account_id(target_account_id)
            .currency(balance.currency)
            .sequence(1u32)
            .layer(Layer::Settled)
            .entry_type("ACCOUNT_SET_ADD_MEMBER_SETTLED_CR")
            .direction(DebitOrCredit::Credit)
            .units(balance.settled_cr_balance)
            .transaction_id(UNASSIGNED_TRANSACTION_ID)
            .build()
            .expect("Couldn't build entry");
        entries.push(entry);
    }
    if balance.settled_dr_balance != Decimal::ZERO {
        let entry = NewEntry::builder()
            .id(EntryId::new())
            .journal_id(balance.journal_id)
            .account_id(target_account_id)
            .currency(balance.currency)
            .sequence(1u32)
            .layer(Layer::Settled)
            .entry_type("ACCOUNT_SET_ADD_MEMBER_SETTLED_DR")
            .direction(DebitOrCredit::Debit)
            .units(balance.settled_dr_balance)
            .transaction_id(UNASSIGNED_TRANSACTION_ID)
            .build()
            .expect("Couldn't build entry");
        entries.push(entry);
    }
    if balance.pending_cr_balance != Decimal::ZERO {
        let entry = NewEntry::builder()
            .id(EntryId::new())
            .journal_id(balance.journal_id)
            .account_id(target_account_id)
            .currency(balance.currency)
            .sequence(1u32)
            .layer(Layer::Pending)
            .entry_type("ACCOUNT_SET_ADD_MEMBER_PENDING_CR")
            .direction(DebitOrCredit::Credit)
            .units(balance.pending_cr_balance)
            .transaction_id(UNASSIGNED_TRANSACTION_ID)
            .build()
            .expect("Couldn't build entry");
        entries.push(entry);
    }
    if balance.pending_dr_balance != Decimal::ZERO {
        let entry = NewEntry::builder()
            .id(EntryId::new())
            .journal_id(balance.journal_id)
            .account_id(target_account_id)
            .currency(balance.currency)
            .sequence(1u32)
            .layer(Layer::Pending)
            .entry_type("ACCOUNT_SET_ADD_MEMBER_PENDING_DR")
            .direction(DebitOrCredit::Debit)
            .units(balance.pending_dr_balance)
            .transaction_id(UNASSIGNED_TRANSACTION_ID)
            .build()
            .expect("Couldn't build entry");
        entries.push(entry);
    }
    if balance.encumbrance_cr_balance != Decimal::ZERO {
        let entry = NewEntry::builder()
            .id(EntryId::new())
            .journal_id(balance.journal_id)
            .account_id(target_account_id)
            .currency(balance.currency)
            .sequence(1u32)
            .layer(Layer::Encumbrance)
            .entry_type("ACCOUNT_SET_ADD_MEMBER_ENCUMBRANCE_CR")
            .direction(DebitOrCredit::Credit)
            .units(balance.encumbrance_cr_balance)
            .transaction_id(UNASSIGNED_TRANSACTION_ID)
            .build()
            .expect("Couldn't build entry");
        entries.push(entry);
    }
    if balance.encumbrance_dr_balance != Decimal::ZERO {
        let entry = NewEntry::builder()
            .id(EntryId::new())
            .journal_id(balance.journal_id)
            .account_id(target_account_id)
            .currency(balance.currency)
            .sequence(1u32)
            .layer(Layer::Encumbrance)
            .entry_type("ACCOUNT_SET_ADD_MEMBER_ENCUMBRANCE_DR")
            .direction(DebitOrCredit::Debit)
            .units(balance.encumbrance_dr_balance)
            .transaction_id(UNASSIGNED_TRANSACTION_ID)
            .build()
            .expect("Couldn't build entry");
        entries.push(entry);
    }
}

fn entries_for_remove_balance(
    entries: &mut Vec<NewEntry>,
    target_account_id: AccountId,
    balance: BalanceSnapshot,
) {
    use rust_decimal::Decimal;

    if balance.settled_cr_balance != Decimal::ZERO {
        let entry = NewEntry::builder()
            .id(EntryId::new())
            .journal_id(balance.journal_id)
            .account_id(target_account_id)
            .currency(balance.currency)
            .sequence(1u32)
            .layer(Layer::Settled)
            .entry_type("ACCOUNT_SET_REMOVE_MEMBER_SETTLED_DR")
            .direction(DebitOrCredit::Debit)
            .units(balance.settled_cr_balance)
            .transaction_id(UNASSIGNED_TRANSACTION_ID)
            .build()
            .expect("Couldn't build entry");
        entries.push(entry);
    }
    if balance.settled_dr_balance != Decimal::ZERO {
        let entry = NewEntry::builder()
            .id(EntryId::new())
            .journal_id(balance.journal_id)
            .account_id(target_account_id)
            .currency(balance.currency)
            .sequence(1u32)
            .layer(Layer::Settled)
            .entry_type("ACCOUNT_SET_REMOVE_MEMBER_SETTLED_CR")
            .direction(DebitOrCredit::Credit)
            .units(balance.settled_dr_balance)
            .transaction_id(UNASSIGNED_TRANSACTION_ID)
            .build()
            .expect("Couldn't build entry");
        entries.push(entry);
    }
    if balance.pending_cr_balance != Decimal::ZERO {
        let entry = NewEntry::builder()
            .id(EntryId::new())
            .journal_id(balance.journal_id)
            .account_id(target_account_id)
            .currency(balance.currency)
            .sequence(1u32)
            .layer(Layer::Pending)
            .entry_type("ACCOUNT_SET_REMOVE_MEMBER_PENDING_DR")
            .direction(DebitOrCredit::Debit)
            .units(balance.pending_cr_balance)
            .transaction_id(UNASSIGNED_TRANSACTION_ID)
            .build()
            .expect("Couldn't build entry");
        entries.push(entry);
    }
    if balance.pending_dr_balance != Decimal::ZERO {
        let entry = NewEntry::builder()
            .id(EntryId::new())
            .journal_id(balance.journal_id)
            .account_id(target_account_id)
            .currency(balance.currency)
            .sequence(1u32)
            .layer(Layer::Pending)
            .entry_type("ACCOUNT_SET_REMOVE_MEMBER_PENDING_CR")
            .direction(DebitOrCredit::Credit)
            .units(balance.pending_dr_balance)
            .transaction_id(UNASSIGNED_TRANSACTION_ID)
            .build()
            .expect("Couldn't build entry");
        entries.push(entry);
    }
    if balance.encumbrance_cr_balance != Decimal::ZERO {
        let entry = NewEntry::builder()
            .id(EntryId::new())
            .journal_id(balance.journal_id)
            .account_id(target_account_id)
            .currency(balance.currency)
            .sequence(1u32)
            .layer(Layer::Encumbrance)
            .entry_type("ACCOUNT_SET_REMOVE_MEMBER_ENCUMBRANCE_DR")
            .direction(DebitOrCredit::Debit)
            .units(balance.encumbrance_cr_balance)
            .transaction_id(UNASSIGNED_TRANSACTION_ID)
            .build()
            .expect("Couldn't build entry");
        entries.push(entry);
    }
    if balance.encumbrance_dr_balance != Decimal::ZERO {
        let entry = NewEntry::builder()
            .id(EntryId::new())
            .journal_id(balance.journal_id)
            .account_id(target_account_id)
            .currency(balance.currency)
            .sequence(1u32)
            .layer(Layer::Encumbrance)
            .entry_type("ACCOUNT_SET_REMOVE_MEMBER_ENCUMBRANCE_CR")
            .direction(DebitOrCredit::Credit)
            .units(balance.encumbrance_dr_balance)
            .transaction_id(UNASSIGNED_TRANSACTION_ID)
            .build()
            .expect("Couldn't build entry");
        entries.push(entry);
    }
}

impl From<&AccountSetEvent> for OutboxEventPayload {
    fn from(event: &AccountSetEvent) -> Self {
        match event {
            #[cfg(feature = "import")]
            AccountSetEvent::Imported {
                source,
                values: account_set,
            } => OutboxEventPayload::AccountSetCreated {
                source: *source,
                account_set: account_set.clone(),
            },
            AccountSetEvent::Initialized {
                values: account_set,
            } => OutboxEventPayload::AccountSetCreated {
                source: DataSource::Local,
                account_set: account_set.clone(),
            },
            AccountSetEvent::Updated { values, fields } => OutboxEventPayload::AccountSetUpdated {
                source: DataSource::Local,
                account_set: values.clone(),
                fields: fields.clone(),
            },
        }
    }
}
