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
    outbox::*,
    primitives::{DataSource, DebitOrCredit, JournalId, Layer},
};

pub use cursor::*;
pub use entity::*;
use error::*;
use repo::*;
pub use repo::{account_set_cursor::*, members_cursor::*};

const UNASSIGNED_TRANSACTION_ID: uuid::Uuid = uuid::Uuid::nil();

#[derive(Clone)]
pub struct AccountSets {
    repo: AccountSetRepo,
    accounts: Accounts,
    entries: Entries,
    balances: Balances,
}

impl AccountSets {
    pub(crate) fn new(
        pool: &PgPool,
        publisher: &OutboxPublisher,
        accounts: &Accounts,
        entries: &Entries,
        balances: &Balances,
    ) -> Self {
        Self {
            repo: AccountSetRepo::new(pool, publisher),
            accounts: accounts.clone(),
            entries: entries.clone(),
            balances: balances.clone(),
        }
    }
    #[instrument(name = "cala_ledger.account_sets.create", skip(self))]
    pub async fn create(
        &self,
        new_account_set: NewAccountSet,
    ) -> Result<AccountSet, AccountSetError> {
        let mut op = self.repo.begin_op().await?;
        let account_set = self.create_in_op(&mut op, new_account_set).await?;
        op.commit().await?;
        Ok(account_set)
    }

    #[instrument(name = "cala_ledger.account_sets.create_in_op", skip(self, db))]
    pub async fn create_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        new_account_set: NewAccountSet,
    ) -> Result<AccountSet, AccountSetError> {
        let new_account = NewAccount::builder()
            .id(new_account_set.id)
            .name(String::new())
            .code(new_account_set.id.to_string())
            .normal_balance_type(new_account_set.normal_balance_type)
            .is_account_set(true)
            .velocity_context_values(new_account_set.context_values())
            .build()
            .expect("Failed to build account");
        self.accounts.create_in_op(db, new_account).await?;

        let account_set = self.repo.create_in_op(db, new_account_set).await?;

        Ok(account_set)
    }

    #[instrument(name = "cala_ledger.account_sets.create_all", skip(self, new_account_sets), fields(count = new_account_sets.len()))]
    pub async fn create_all(
        &self,
        new_account_sets: Vec<NewAccountSet>,
    ) -> Result<Vec<AccountSet>, AccountSetError> {
        let mut op = self.repo.begin_op().await?;
        let account_sets = self.create_all_in_op(&mut op, new_account_sets).await?;
        op.commit().await?;
        Ok(account_sets)
    }

    #[instrument(name = "cala_ledger.account_sets.create_all_in_op", skip(self, db))]
    pub async fn create_all_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        new_account_sets: Vec<NewAccountSet>,
    ) -> Result<Vec<AccountSet>, AccountSetError> {
        let mut new_accounts = Vec::new();
        for new_account_set in new_account_sets.iter() {
            let new_account = NewAccount::builder()
                .id(new_account_set.id)
                .name(String::new())
                .code(new_account_set.id.to_string())
                .normal_balance_type(new_account_set.normal_balance_type)
                .is_account_set(true)
                .velocity_context_values(new_account_set.context_values())
                .build()
                .expect("Failed to build account");
            new_accounts.push(new_account);
        }
        self.accounts.create_all_in_op(db, new_accounts).await?;

        let account_sets = self.repo.create_all_in_op(db, new_account_sets).await?;

        Ok(account_sets)
    }

    #[instrument(name = "cala_ledger.account_sets.persist", skip(self, account_set))]
    pub async fn persist(&self, account_set: &mut AccountSet) -> Result<(), AccountSetError> {
        let mut op = self.repo.begin_op().await?;
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
        db: &mut impl es_entity::AtomicOperation,
        account_set: &mut AccountSet,
    ) -> Result<(), AccountSetError> {
        self.repo.update_in_op(db, account_set).await?;

        self.accounts
            .update_velocity_context_values_in_op(db, account_set.values())
            .await?;

        Ok(())
    }

    #[instrument(name = "cala_ledger.account_sets.add_member", skip(self, member), fields(account_set_id = %account_set_id))]
    pub async fn add_member(
        &self,
        account_set_id: AccountSetId,
        member: impl Into<AccountSetMemberId>,
    ) -> Result<AccountSet, AccountSetError> {
        let mut op = self.repo.begin_op().await?;
        let account_set = self
            .add_member_in_op(&mut op, account_set_id, member)
            .await?;
        op.commit().await?;
        Ok(account_set)
    }

    #[instrument(name = "cala_ledger.account_sets.add_member_in_op", skip(self, op, member), fields(account_set_id = %account_set_id, is_account = tracing::field::Empty, is_account_set = tracing::field::Empty, member_id = tracing::field::Empty))]
    pub async fn add_member_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        account_set_id: AccountSetId,
        member: impl Into<AccountSetMemberId>,
    ) -> Result<AccountSet, AccountSetError> {
        let member = member.into();
        let (time, parents, account_set, member_id) = match member {
            AccountSetMemberId::Account(id) => {
                tracing::Span::current().record("is_account", true);
                tracing::Span::current().record("is_account_set", false);
                tracing::Span::current().record("member_id", tracing::field::display(&id));
                let set = self.repo.find_by_id_in_op(&mut *op, account_set_id).await?;
                let (time, parents) = self
                    .repo
                    .add_member_account_and_return_parents(&mut *op, account_set_id, id)
                    .await?;
                (time, parents, set, id)
            }
            AccountSetMemberId::AccountSet(id) => {
                tracing::Span::current().record("is_account", false);
                tracing::Span::current().record("is_account_set", true);
                tracing::Span::current().record("member_id", tracing::field::display(&id));
                let mut accounts = self
                    .repo
                    .find_all_in_op::<AccountSet>(&mut *op, &[account_set_id, id])
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
                    .add_member_set_and_return_parents(op, account_set_id, id)
                    .await?;
                (time, parents, target, AccountId::from(id))
            }
        };

        self.repo.publisher.publish_all(op, std::iter::once(OutboxEventPayload::AccountSetMemberCreated {
            source: DataSource::Local,
            account_set_id,
            member_id: member,
        })).await?;

        let balances = self
            .balances
            .find_balances_for_update(op, account_set.values().journal_id, member_id)
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
            .update_balances_in_op(
                op,
                account_set.values().journal_id,
                entries,
                time.date_naive(),
                time,
                mappings,
            )
            .await?;

        Ok(account_set)
    }

    #[instrument(name = "cala_ledger.account_sets.remove_member", skip(self, member), fields(account_set_id = %account_set_id))]
    pub async fn remove_member(
        &self,
        account_set_id: AccountSetId,
        member: impl Into<AccountSetMemberId>,
    ) -> Result<AccountSet, AccountSetError> {
        let mut op = self.repo.begin_op().await?;
        let account_set = self
            .remove_member_in_op(&mut op, account_set_id, member)
            .await?;
        op.commit().await?;
        Ok(account_set)
    }

    #[instrument(name = "cala_ledger.account_sets.remove_member_in_op", skip(self, op, member), fields(account_set_id = %account_set_id))]
    pub async fn remove_member_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        account_set_id: AccountSetId,
        member: impl Into<AccountSetMemberId>,
    ) -> Result<AccountSet, AccountSetError> {
        let member = member.into();
        let (time, parents, account_set, member_id) = match member {
            AccountSetMemberId::Account(id) => {
                let set = self.repo.find_by_id_in_op(&mut *op, account_set_id).await?;
                let (time, parents) = self
                    .repo
                    .remove_member_account_and_return_parents(op, account_set_id, id)
                    .await?;
                (time, parents, set, id)
            }
            AccountSetMemberId::AccountSet(id) => {
                let mut accounts = self
                    .repo
                    .find_all_in_op::<AccountSet>(&mut *op, &[account_set_id, id])
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
                    .remove_member_set_and_return_parents(op, account_set_id, id)
                    .await?;
                (time, parents, target, AccountId::from(id))
            }
        };

        self.repo.publisher.publish_all(op, std::iter::once(OutboxEventPayload::AccountSetMemberRemoved {
            source: DataSource::Local,
            account_set_id,
            member_id: member,
        })).await?;

        let balances = self
            .balances
            .find_balances_for_update(op, account_set.values().journal_id, member_id)
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
            .update_balances_in_op(
                op,
                account_set.values().journal_id,
                entries,
                time.date_naive(),
                time,
                mappings,
            )
            .await?;

        Ok(account_set)
    }

    #[instrument(name = "cala_ledger.account_sets.find_all", skip(self))]
    pub async fn find_all<T: From<AccountSet>>(
        &self,
        account_set_ids: &[AccountSetId],
    ) -> Result<HashMap<AccountSetId, T>, AccountSetError> {
        self.repo.find_all(account_set_ids).await
    }

    #[instrument(name = "cala_ledger.account_sets.find_all_in_op", skip(self, op))]
    pub async fn find_all_in_op<T: From<AccountSet>>(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        account_set_ids: &[AccountSetId],
    ) -> Result<HashMap<AccountSetId, T>, AccountSetError> {
        self.repo.find_all_in_op(op, account_set_ids).await
    }

    #[instrument(name = "cala_ledger.account_sets.find", skip(self))]
    pub async fn find(&self, account_set_id: AccountSetId) -> Result<AccountSet, AccountSetError> {
        self.repo.find_by_id(account_set_id).await
    }

    #[instrument(name = "cala_ledger.account_sets.find_in_op", skip(self, op))]
    pub async fn find_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        account_set_id: AccountSetId,
    ) -> Result<AccountSet, AccountSetError> {
        self.repo.find_by_id_in_op(op, account_set_id).await
    }

    #[instrument(name = "cala_ledger.accounts_sets.find_by_external_id", skip(self))]
    pub async fn find_by_external_id(
        &self,
        external_id: String,
    ) -> Result<AccountSet, AccountSetError> {
        self.repo.find_by_external_id(Some(external_id)).await
    }

    #[instrument(name = "cala_ledger.account_sets.find_where_member", skip(self))]
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

    #[instrument(name = "cala_ledger.account_sets.list_for_name", skip(self))]
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

    #[instrument(name = "cala_ledger.account_sets.list_for_name_in_op", skip(self, op))]
    pub async fn list_for_name_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        name: String,
        args: es_entity::PaginatedQueryArgs<AccountSetsByCreatedAtCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<AccountSet, AccountSetsByCreatedAtCursor>,
        AccountSetError,
    > {
        self.repo
            .list_for_name_by_created_at_in_op(op, name, args, Default::default())
            .await
    }

    #[instrument(
        name = "cala_ledger.account_sets.find_where_member_in_op",
        skip(self, op)
    )]
    pub async fn find_where_member_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        member: impl Into<AccountSetMemberId> + std::fmt::Debug,
        query: es_entity::PaginatedQueryArgs<AccountSetsByNameCursor>,
    ) -> Result<es_entity::PaginatedQueryRet<AccountSet, AccountSetsByNameCursor>, AccountSetError>
    {
        match member.into() {
            AccountSetMemberId::Account(account_id) => {
                self.repo
                    .find_where_account_is_member_in_op(op, account_id, query)
                    .await
            }
            AccountSetMemberId::AccountSet(account_set_id) => {
                self.repo
                    .find_where_account_set_is_member_in_op(op, account_set_id, query)
                    .await
            }
        }
    }

    pub async fn list_members_by_created_at(
        &self,
        id: AccountSetId,
        args: es_entity::PaginatedQueryArgs<AccountSetMembersByCreatedAtCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<AccountSetMember, AccountSetMembersByCreatedAtCursor>,
        AccountSetError,
    > {
        self.repo.list_children_by_created_at(id, args).await
    }

    pub async fn list_members_by_created_at_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        id: AccountSetId,
        args: es_entity::PaginatedQueryArgs<AccountSetMembersByCreatedAtCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<AccountSetMember, AccountSetMembersByCreatedAtCursor>,
        AccountSetError,
    > {
        self.repo
            .list_children_by_created_at_in_op(op, id, args)
            .await
    }

    pub async fn list_members_by_external_id(
        &self,
        id: AccountSetId,
        args: es_entity::PaginatedQueryArgs<AccountSetMembersByExternalIdCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<
            AccountSetMemberByExternalId,
            AccountSetMembersByExternalIdCursor,
        >,
        AccountSetError,
    > {
        self.repo.list_children_by_external_id(id, args).await
    }

    pub async fn list_members_by_external_id_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        id: AccountSetId,
        args: es_entity::PaginatedQueryArgs<AccountSetMembersByExternalIdCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<
            AccountSetMemberByExternalId,
            AccountSetMembersByExternalIdCursor,
        >,
        AccountSetError,
    > {
        self.repo
            .list_children_by_external_id_in_op(op, id, args)
            .await
    }

    pub(crate) async fn fetch_mappings_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        journal_id: JournalId,
        account_ids: &[AccountId],
    ) -> Result<HashMap<AccountId, Vec<AccountSetId>>, AccountSetError> {
        self.repo
            .fetch_mappings_in_op(op, journal_id, account_ids)
            .await
    }

    #[cfg(feature = "import")]
    pub(crate) async fn sync_account_set_creation(
        &self,
        mut db: es_entity::DbOpWithTime<'_>,
        origin: DataSourceId,
        values: AccountSetValues,
    ) -> Result<(), AccountSetError> {
        let mut account_set = AccountSet::import(origin, values);
        self.repo
            .import_in_op(&mut db, origin, &mut account_set)
            .await?;
        Ok(())
    }

    #[cfg(feature = "import")]
    pub(crate) async fn sync_account_set_update(
        &self,
        mut db: es_entity::DbOpWithTime<'_>,
        values: AccountSetValues,
        fields: Vec<String>,
    ) -> Result<(), AccountSetError> {
        let mut account_set = self.repo.find_by_id(values.id).await?;
        let _ = account_set.update((values, fields));
        self.repo.update_in_op(&mut db, &mut account_set).await?;
        Ok(())
    }

    #[cfg(feature = "import")]
    pub(crate) async fn sync_account_set_member_creation(
        &self,
        mut db: es_entity::DbOpWithTime<'_>,
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
        self.repo.publisher.publish_all(&mut db, std::iter::once(OutboxEventPayload::AccountSetMemberCreated {
            source: DataSource::Remote { id: origin },
            account_set_id,
            member_id,
        })).await?;
        Ok(())
    }

    #[cfg(feature = "import")]
    pub(crate) async fn sync_account_set_member_removal(
        &self,
        mut db: es_entity::DbOpWithTime<'_>,
        origin: DataSourceId,
        account_set_id: AccountSetId,
        member_id: AccountSetMemberId,
    ) -> Result<(), AccountSetError> {
        match member_id {
            AccountSetMemberId::Account(account_id) => {
                self.repo
                    .import_remove_member_account(&mut db, account_set_id, account_id)
                    .await?;
            }
            AccountSetMemberId::AccountSet(account_set_id) => {
                self.repo
                    .import_remove_member_set(&mut db, account_set_id, account_set_id)
                    .await?;
            }
        }
        self.repo.publisher.publish_all(&mut db, std::iter::once(OutboxEventPayload::AccountSetMemberRemoved {
            source: DataSource::Remote { id: origin },
            account_set_id,
            member_id,
        })).await?;
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
        let source = es_entity::context::EventContext::current()
            .data()
            .lookup("data_source")
            .ok()
            .flatten()
            .unwrap_or(DataSource::Local);

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
                source,
                account_set: account_set.clone(),
            },
            AccountSetEvent::Updated { values, fields } => OutboxEventPayload::AccountSetUpdated {
                source,
                account_set: values.clone(),
                fields: fields.clone(),
            },
        }
    }
}
