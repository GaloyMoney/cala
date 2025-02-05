mod cursor;
mod entity;
pub mod error;
mod repo;

use sqlx::PgPool;
use std::collections::HashMap;
use tracing::instrument;

#[cfg(feature = "import")]
use crate::primitives::DataSourceId;
use crate::{
    account::*,
    balance::*,
    entry::*,
    ledger_operation::*,
    outbox::*,
    primitives::{DataSource, DebitOrCredit, JournalId, Layer},
};

pub use cursor::*;
pub use entity::*;
use error::*;
use repo::*;
pub use repo::{account_set_cursor::*, members_cursor::*};

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
        let mut op = LedgerOperation::init(&self.pool, &self.outbox).await?;
        let account_set = self.create_in_op(&mut op, new_account_set).await?;
        op.commit().await?;
        Ok(account_set)
    }

    #[instrument(name = "cala_ledger.account_sets.create", skip(self, db))]
    pub async fn create_in_op(
        &self,
        db: &mut LedgerOperation<'_>,
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
        self.accounts.create_in_op(db, new_account).await?;
        let account_set = self.repo.create_in_op(db.op(), new_account_set).await?;
        db.accumulate(account_set.events.last_persisted(1).map(|p| &p.event));
        Ok(account_set)
    }

    #[instrument(name = "cala_ledger.account_sets.persist", skip(self, account_set))]
    pub async fn persist(&self, account_set: &mut AccountSet) -> Result<(), AccountSetError> {
        let mut op = LedgerOperation::init(&self.pool, &self.outbox).await?;
        self.persist_in_op(&mut op, account_set).await?;
        op.commit().await?;
        Ok(())
    }

    #[instrument(
        name = "cala_ledger.account_sets.persist_in_op",
        skip(self, db, account_set)
    )]
    pub async fn persist_in_op(
        &self,
        db: &mut LedgerOperation<'_>,
        account_set: &mut AccountSet,
    ) -> Result<(), AccountSetError> {
        let n_events = self.repo.update_in_op(db.op(), account_set).await?;
        db.accumulate(
            account_set
                .events
                .last_persisted(n_events)
                .map(|p| &p.event),
        );
        Ok(())
    }

    pub async fn add_member(
        &self,
        account_set_id: AccountSetId,
        member: impl Into<AccountSetMemberId>,
    ) -> Result<AccountSet, AccountSetError> {
        let mut op = LedgerOperation::init(&self.pool, &self.outbox).await?;
        let account_set = self
            .add_member_in_op(&mut op, account_set_id, member)
            .await?;
        op.commit().await?;
        Ok(account_set)
    }

    pub async fn add_member_in_op(
        &self,
        op: &mut LedgerOperation<'_>,
        account_set_id: AccountSetId,
        member: impl Into<AccountSetMemberId>,
    ) -> Result<AccountSet, AccountSetError> {
        let member = member.into();
        let (time, parents, account_set, member_id) = match member {
            AccountSetMemberId::Account(id) => {
                let set = self.repo.find_by_id_in_tx(op.tx(), account_set_id).await?;
                let (time, parents) = self
                    .repo
                    .add_member_account_and_return_parents(op.tx(), account_set_id, id)
                    .await?;
                (time, parents, set, id)
            }
            AccountSetMemberId::AccountSet(id) => {
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
                member_id: member,
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
        member: impl Into<AccountSetMemberId>,
    ) -> Result<AccountSet, AccountSetError> {
        let mut op = LedgerOperation::init(&self.pool, &self.outbox).await?;
        let account_set = self
            .remove_member_in_op(&mut op, account_set_id, member)
            .await?;
        op.commit().await?;
        Ok(account_set)
    }

    pub async fn remove_member_in_op(
        &self,
        op: &mut LedgerOperation<'_>,
        account_set_id: AccountSetId,
        member: impl Into<AccountSetMemberId>,
    ) -> Result<AccountSet, AccountSetError> {
        let member = member.into();
        let (time, parents, account_set, member_id) = match member {
            AccountSetMemberId::Account(id) => {
                let set = self.repo.find_by_id_in_tx(op.tx(), account_set_id).await?;
                let (time, parents) = self
                    .repo
                    .remove_member_account_and_return_parents(op.tx(), account_set_id, id)
                    .await?;
                (time, parents, set, id)
            }
            AccountSetMemberId::AccountSet(id) => {
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
                member_id: member,
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

    #[instrument(name = "cala_ledger.account_sets.find_all", skip(self, op), err)]
    pub async fn find_all_in_op<T: From<AccountSet>>(
        &self,
        op: &mut LedgerOperation<'_>,
        account_set_ids: &[AccountSetId],
    ) -> Result<HashMap<AccountSetId, T>, AccountSetError> {
        self.repo.find_all_in_tx(op.tx(), account_set_ids).await
    }

    #[instrument(name = "cala_ledger.account_sets.find", skip(self), err)]
    pub async fn find(&self, account_set_id: AccountSetId) -> Result<AccountSet, AccountSetError> {
        self.repo.find_by_id(account_set_id).await
    }

    #[instrument(
        name = "cala_ledger.accounts_sets.find_by_external_id",
        skip(self),
        err
    )]
    pub async fn find_by_external_id(
        &self,
        external_id: String,
    ) -> Result<AccountSet, AccountSetError> {
        self.repo.find_by_external_id(Some(external_id)).await
    }

    #[instrument(name = "cala_ledger.account_sets.find_where_member", skip(self), err)]
    pub async fn find_where_member(
        &self,
        member: impl Into<AccountSetMemberId> + std::fmt::Debug,
        query: es_entity::PaginatedQueryArgs<AccountSetsByNameCursor>,
    ) -> Result<es_entity::PaginatedQueryRet<AccountSet, AccountSetsByNameCursor>, AccountSetError>
    {
        match member.into() {
            AccountSetMemberId::Account(account_id) => {
                self.repo
                    .find_where_account_is_member(account_id, query)
                    .await
            }
            AccountSetMemberId::AccountSet(account_set_id) => {
                self.repo
                    .find_where_account_set_is_member(account_set_id, query)
                    .await
            }
        }
    }

    #[instrument(name = "cala_ledger.account_sets.list_for_name", skip(self), err)]
    pub async fn list_for_name(
        &self,
        name: String,
        args: es_entity::PaginatedQueryArgs<AccountSetsByCreatedAtCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<AccountSet, AccountSetsByCreatedAtCursor>,
        AccountSetError,
    > {
        self.repo
            .list_for_name_by_created_at(name, args, Default::default())
            .await
    }

    #[instrument(
        name = "cala_ledger.account_sets.list_for_name_in_op",
        skip(self, op),
        err
    )]
    pub async fn list_for_name_in_op(
        &self,
        op: &mut LedgerOperation<'_>,
        name: String,
        args: es_entity::PaginatedQueryArgs<AccountSetsByCreatedAtCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<AccountSet, AccountSetsByCreatedAtCursor>,
        AccountSetError,
    > {
        self.repo
            .list_for_name_by_created_at_in_tx(op.tx(), name, args, Default::default())
            .await
    }

    #[instrument(
        name = "cala_ledger.account_sets.find_where_member_in_op",
        skip(self, op),
        err
    )]
    pub async fn find_where_member_in_op(
        &self,
        op: &mut LedgerOperation<'_>,
        member: impl Into<AccountSetMemberId> + std::fmt::Debug,
        query: es_entity::PaginatedQueryArgs<AccountSetsByNameCursor>,
    ) -> Result<es_entity::PaginatedQueryRet<AccountSet, AccountSetsByNameCursor>, AccountSetError>
    {
        match member.into() {
            AccountSetMemberId::Account(account_id) => {
                self.repo
                    .find_where_account_is_member_in_tx(op.tx(), account_id, query)
                    .await
            }
            AccountSetMemberId::AccountSet(account_set_id) => {
                self.repo
                    .find_where_account_set_is_member_in_tx(op.tx(), account_set_id, query)
                    .await
            }
        }
    }

    pub async fn list_members(
        &self,
        id: AccountSetId,
        args: es_entity::PaginatedQueryArgs<AccountSetMembersCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<AccountSetMember, AccountSetMembersCursor>,
        AccountSetError,
    > {
        self.repo.list_children(id, args).await
    }

    pub async fn list_members_in_op(
        &self,
        op: &mut LedgerOperation<'_>,
        id: AccountSetId,
        args: es_entity::PaginatedQueryArgs<AccountSetMembersCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<AccountSetMember, AccountSetMembersCursor>,
        AccountSetError,
    > {
        self.repo.list_children_in_tx(op.tx(), id, args).await
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
        mut db: es_entity::DbOp<'_>,
        origin: DataSourceId,
        values: AccountSetValues,
    ) -> Result<(), AccountSetError> {
        let mut account_set = AccountSet::import(origin, values);
        self.repo
            .import_in_op(&mut db, origin, &mut account_set)
            .await?;
        let recorded_at = db.now();
        let outbox_events: Vec<_> = account_set
            .events
            .last_persisted(1)
            .map(|p| OutboxEventPayload::from(&p.event))
            .collect();
        self.outbox
            .persist_events_at(db.into_tx(), outbox_events, recorded_at)
            .await?;
        Ok(())
    }

    #[cfg(feature = "import")]
    pub async fn sync_account_set_update(
        &self,
        mut db: es_entity::DbOp<'_>,
        values: AccountSetValues,
        fields: Vec<String>,
    ) -> Result<(), AccountSetError> {
        let mut account_set = self.repo.find_by_id(values.id).await?;
        account_set.update((values, fields));
        let n_events = self.repo.update_in_op(&mut db, &mut account_set).await?;
        let recorded_at = db.now();
        let outbox_events: Vec<_> = account_set
            .events
            .last_persisted(n_events)
            .map(|p| OutboxEventPayload::from(&p.event))
            .collect();
        self.outbox
            .persist_events_at(db.into_tx(), outbox_events, recorded_at)
            .await?;
        Ok(())
    }

    #[cfg(feature = "import")]
    pub async fn sync_account_set_member_creation(
        &self,
        mut db: es_entity::DbOp<'_>,
        origin: DataSourceId,
        account_set_id: AccountSetId,
        member_id: AccountSetMemberId,
    ) -> Result<(), AccountSetError> {
        match member_id {
            AccountSetMemberId::Account(account_id) => {
                self.repo
                    .import_member_account_in_op(&mut db, account_set_id, account_id)
                    .await?;
            }
            AccountSetMemberId::AccountSet(account_set_id) => {
                self.repo
                    .import_member_set_in_op(&mut db, account_set_id, account_set_id)
                    .await?;
            }
        }
        let recorded_at = db.now();
        self.outbox
            .persist_events_at(
                db.into_tx(),
                std::iter::once(OutboxEventPayload::AccountSetMemberCreated {
                    source: DataSource::Remote { id: origin },
                    account_set_id,
                    member_id,
                }),
                recorded_at,
            )
            .await?;
        Ok(())
    }

    #[cfg(feature = "import")]
    pub async fn sync_account_set_member_removal(
        &self,
        mut db: es_entity::DbOp<'_>,
        origin: DataSourceId,
        account_set_id: AccountSetId,
        member_id: AccountSetMemberId,
    ) -> Result<(), AccountSetError> {
        match member_id {
            AccountSetMemberId::Account(account_id) => {
                self.repo
                    .import_remove_member_account(db.tx(), account_set_id, account_id)
                    .await?;
            }
            AccountSetMemberId::AccountSet(account_set_id) => {
                self.repo
                    .import_remove_member_set(db.tx(), account_set_id, account_set_id)
                    .await?;
            }
        }
        let recorded_at = db.now();
        self.outbox
            .persist_events_at(
                db.into_tx(),
                std::iter::once(OutboxEventPayload::AccountSetMemberRemoved {
                    source: DataSource::Remote { id: origin },
                    account_set_id,
                    member_id,
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

    if balance.settled.cr_balance != Decimal::ZERO {
        let entry = NewEntry::builder()
            .id(EntryId::new())
            .journal_id(balance.journal_id)
            .account_id(target_account_id)
            .currency(balance.currency)
            .sequence(1u32)
            .layer(Layer::Settled)
            .entry_type("ACCOUNT_SET_ADD_MEMBER_SETTLED_CR")
            .direction(DebitOrCredit::Credit)
            .units(balance.settled.cr_balance)
            .transaction_id(UNASSIGNED_TRANSACTION_ID)
            .build()
            .expect("Couldn't build entry");
        entries.push(entry);
    }
    if balance.settled.dr_balance != Decimal::ZERO {
        let entry = NewEntry::builder()
            .id(EntryId::new())
            .journal_id(balance.journal_id)
            .account_id(target_account_id)
            .currency(balance.currency)
            .sequence(1u32)
            .layer(Layer::Settled)
            .entry_type("ACCOUNT_SET_ADD_MEMBER_SETTLED_DR")
            .direction(DebitOrCredit::Debit)
            .units(balance.settled.dr_balance)
            .transaction_id(UNASSIGNED_TRANSACTION_ID)
            .build()
            .expect("Couldn't build entry");
        entries.push(entry);
    }
    if balance.pending.cr_balance != Decimal::ZERO {
        let entry = NewEntry::builder()
            .id(EntryId::new())
            .journal_id(balance.journal_id)
            .account_id(target_account_id)
            .currency(balance.currency)
            .sequence(1u32)
            .layer(Layer::Pending)
            .entry_type("ACCOUNT_SET_ADD_MEMBER_PENDING_CR")
            .direction(DebitOrCredit::Credit)
            .units(balance.pending.cr_balance)
            .transaction_id(UNASSIGNED_TRANSACTION_ID)
            .build()
            .expect("Couldn't build entry");
        entries.push(entry);
    }
    if balance.pending.dr_balance != Decimal::ZERO {
        let entry = NewEntry::builder()
            .id(EntryId::new())
            .journal_id(balance.journal_id)
            .account_id(target_account_id)
            .currency(balance.currency)
            .sequence(1u32)
            .layer(Layer::Pending)
            .entry_type("ACCOUNT_SET_ADD_MEMBER_PENDING_DR")
            .direction(DebitOrCredit::Debit)
            .units(balance.pending.dr_balance)
            .transaction_id(UNASSIGNED_TRANSACTION_ID)
            .build()
            .expect("Couldn't build entry");
        entries.push(entry);
    }
    if balance.encumbrance.cr_balance != Decimal::ZERO {
        let entry = NewEntry::builder()
            .id(EntryId::new())
            .journal_id(balance.journal_id)
            .account_id(target_account_id)
            .currency(balance.currency)
            .sequence(1u32)
            .layer(Layer::Encumbrance)
            .entry_type("ACCOUNT_SET_ADD_MEMBER_ENCUMBRANCE_CR")
            .direction(DebitOrCredit::Credit)
            .units(balance.encumbrance.cr_balance)
            .transaction_id(UNASSIGNED_TRANSACTION_ID)
            .build()
            .expect("Couldn't build entry");
        entries.push(entry);
    }
    if balance.encumbrance.dr_balance != Decimal::ZERO {
        let entry = NewEntry::builder()
            .id(EntryId::new())
            .journal_id(balance.journal_id)
            .account_id(target_account_id)
            .currency(balance.currency)
            .sequence(1u32)
            .layer(Layer::Encumbrance)
            .entry_type("ACCOUNT_SET_ADD_MEMBER_ENCUMBRANCE_DR")
            .direction(DebitOrCredit::Debit)
            .units(balance.encumbrance.dr_balance)
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

    if balance.settled.cr_balance != Decimal::ZERO {
        let entry = NewEntry::builder()
            .id(EntryId::new())
            .journal_id(balance.journal_id)
            .account_id(target_account_id)
            .currency(balance.currency)
            .sequence(1u32)
            .layer(Layer::Settled)
            .entry_type("ACCOUNT_SET_REMOVE_MEMBER_SETTLED_DR")
            .direction(DebitOrCredit::Debit)
            .units(balance.settled.cr_balance)
            .transaction_id(UNASSIGNED_TRANSACTION_ID)
            .build()
            .expect("Couldn't build entry");
        entries.push(entry);
    }
    if balance.settled.dr_balance != Decimal::ZERO {
        let entry = NewEntry::builder()
            .id(EntryId::new())
            .journal_id(balance.journal_id)
            .account_id(target_account_id)
            .currency(balance.currency)
            .sequence(1u32)
            .layer(Layer::Settled)
            .entry_type("ACCOUNT_SET_REMOVE_MEMBER_SETTLED_CR")
            .direction(DebitOrCredit::Credit)
            .units(balance.settled.dr_balance)
            .transaction_id(UNASSIGNED_TRANSACTION_ID)
            .build()
            .expect("Couldn't build entry");
        entries.push(entry);
    }
    if balance.pending.cr_balance != Decimal::ZERO {
        let entry = NewEntry::builder()
            .id(EntryId::new())
            .journal_id(balance.journal_id)
            .account_id(target_account_id)
            .currency(balance.currency)
            .sequence(1u32)
            .layer(Layer::Pending)
            .entry_type("ACCOUNT_SET_REMOVE_MEMBER_PENDING_DR")
            .direction(DebitOrCredit::Debit)
            .units(balance.pending.cr_balance)
            .transaction_id(UNASSIGNED_TRANSACTION_ID)
            .build()
            .expect("Couldn't build entry");
        entries.push(entry);
    }
    if balance.pending.dr_balance != Decimal::ZERO {
        let entry = NewEntry::builder()
            .id(EntryId::new())
            .journal_id(balance.journal_id)
            .account_id(target_account_id)
            .currency(balance.currency)
            .sequence(1u32)
            .layer(Layer::Pending)
            .entry_type("ACCOUNT_SET_REMOVE_MEMBER_PENDING_CR")
            .direction(DebitOrCredit::Credit)
            .units(balance.pending.dr_balance)
            .transaction_id(UNASSIGNED_TRANSACTION_ID)
            .build()
            .expect("Couldn't build entry");
        entries.push(entry);
    }
    if balance.encumbrance.cr_balance != Decimal::ZERO {
        let entry = NewEntry::builder()
            .id(EntryId::new())
            .journal_id(balance.journal_id)
            .account_id(target_account_id)
            .currency(balance.currency)
            .sequence(1u32)
            .layer(Layer::Encumbrance)
            .entry_type("ACCOUNT_SET_REMOVE_MEMBER_ENCUMBRANCE_DR")
            .direction(DebitOrCredit::Debit)
            .units(balance.encumbrance.cr_balance)
            .transaction_id(UNASSIGNED_TRANSACTION_ID)
            .build()
            .expect("Couldn't build entry");
        entries.push(entry);
    }
    if balance.encumbrance.dr_balance != Decimal::ZERO {
        let entry = NewEntry::builder()
            .id(EntryId::new())
            .journal_id(balance.journal_id)
            .account_id(target_account_id)
            .currency(balance.currency)
            .sequence(1u32)
            .layer(Layer::Encumbrance)
            .entry_type("ACCOUNT_SET_REMOVE_MEMBER_ENCUMBRANCE_CR")
            .direction(DebitOrCredit::Credit)
            .units(balance.encumbrance.dr_balance)
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
