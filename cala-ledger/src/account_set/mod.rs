mod entity;
pub mod error;
mod repo;

use es_entity::clock::ClockHandle;
use sqlx::PgPool;
use std::collections::HashMap;
use tracing::instrument;

use crate::{account::*, balance::*, outbox::*, primitives::JournalId};

pub use entity::*;
use error::*;
use repo::*;
pub use repo::{account_set_cursor::*, members_cursor::*};

#[derive(Clone)]
pub struct AccountSets {
    repo: AccountSetRepo,
    accounts: Accounts,
    balances: Balances,
    clock: ClockHandle,
}

impl AccountSets {
    pub(crate) fn new(
        pool: &PgPool,
        publisher: &OutboxPublisher,
        accounts: &Accounts,
        balances: &Balances,
        clock: &ClockHandle,
    ) -> Self {
        Self {
            repo: AccountSetRepo::new(pool, publisher),
            accounts: accounts.clone(),
            balances: balances.clone(),
            clock: clock.clone(),
        }
    }
    #[instrument(name = "cala_ledger.account_sets.create", skip(self))]
    pub async fn create(
        &self,
        new_account_set: NewAccountSet,
    ) -> Result<AccountSet, AccountSetError> {
        let mut op = self.repo.begin_op_with_clock(&self.clock).await?;
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
            .eventually_consistent(new_account_set.is_eventually_consistent())
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
        let mut op = self.repo.begin_op_with_clock(&self.clock).await?;
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
                .eventually_consistent(new_account_set.is_eventually_consistent())
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
        let mut op = self.repo.begin_op_with_clock(&self.clock).await?;
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
        let mut op = self.repo.begin_op_with_clock(&self.clock).await?;
        let account_set = self
            .add_member_in_op(&mut op, account_set_id, member)
            .await?;
        op.commit().await?;
        Ok(account_set)
    }

    #[instrument(
        name = "cala_ledger.account_sets.add_member_in_op",
        skip(self, op, member),
        fields(
            account_set_id = %account_set_id,
            is_account = tracing::field::Empty,
            is_account_set = tracing::field::Empty,
            member_id = tracing::field::Empty,
        ),
        err(level = "warn")
    )]
    pub async fn add_member_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        account_set_id: AccountSetId,
        member: impl Into<AccountSetMemberId>,
    ) -> Result<AccountSet, AccountSetError> {
        let member = member.into();

        // Resolve the target set (and, for set-member, verify the journal
        // matches) without writing the membership row, so we can run the
        // no-history check first.
        let (account_set, member_id) = match member {
            AccountSetMemberId::Account(id) => {
                tracing::Span::current().record("is_account", true);
                tracing::Span::current().record("is_account_set", false);
                tracing::Span::current().record("member_id", tracing::field::display(&id));
                let set = self.repo.find_by_id_in_op(&mut *op, account_set_id).await?;
                (set, id)
            }
            AccountSetMemberId::AccountSet(id) => {
                tracing::Span::current().record("is_account", false);
                tracing::Span::current().record("is_account_set", true);
                tracing::Span::current().record("member_id", tracing::field::display(&id));
                let mut sets = self
                    .repo
                    .find_all_in_op::<AccountSet>(&mut *op, &[account_set_id, id])
                    .await?;
                let target = sets
                    .remove(&account_set_id)
                    .ok_or(AccountSetError::CouldNotFindById(account_set_id))?;
                let member_set = sets
                    .remove(&id)
                    .ok_or(AccountSetError::CouldNotFindById(id))?;

                if target.values().journal_id != member_set.values().journal_id {
                    return Err(AccountSetError::JournalIdMismatch);
                }

                (target, AccountId::from(id))
            }
        };

        self.assert_member_history_empty_in_op(
            op,
            account_set_id,
            account_set.values().journal_id,
            AccountId::from(&account_set.id()),
            member_id,
        )
        .await?;

        match member {
            AccountSetMemberId::Account(id) => {
                self.repo
                    .add_member_account_and_return_parents(&mut *op, account_set_id, id)
                    .await?;
            }
            AccountSetMemberId::AccountSet(id) => {
                self.repo
                    .add_member_set_and_return_parents(op, account_set_id, id)
                    .await?;
            }
        }

        Ok(account_set)
    }

    /// Refuse the membership change if `member_id` already has any
    /// `cala_balance_history` row in `journal_id`. Folding existing
    /// balance into a parent set after the fact is unsafe: the streaming
    /// rollup only folds in a member's activity from the point it joins,
    /// so pre-existing balance would be silently dropped, and the
    /// symmetric remove case has no safe unfold path either, so we forbid
    /// both.
    ///
    /// The check itself is run under exclusive locks on the parent set
    /// and the candidate member in the EC-set lock namespace, so the
    /// existence query reflects committed state even with concurrent
    /// posters in flight.
    async fn assert_member_history_empty_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        account_set_id: AccountSetId,
        journal_id: JournalId,
        target_account_id: AccountId,
        member_id: AccountId,
    ) -> Result<(), AccountSetError> {
        if self
            .balances
            .member_has_balance_history_in_op(op, journal_id, target_account_id, member_id)
            .await?
        {
            return Err(AccountSetError::MemberHasBalanceHistory {
                account_set_id,
                member_id,
            });
        }
        Ok(())
    }

    #[instrument(name = "cala_ledger.account_sets.remove_member", skip(self, member), fields(account_set_id = %account_set_id))]
    pub async fn remove_member(
        &self,
        account_set_id: AccountSetId,
        member: impl Into<AccountSetMemberId>,
    ) -> Result<AccountSet, AccountSetError> {
        let mut op = self.repo.begin_op_with_clock(&self.clock).await?;
        let account_set = self
            .remove_member_in_op(&mut op, account_set_id, member)
            .await?;
        op.commit().await?;
        Ok(account_set)
    }

    #[instrument(
        name = "cala_ledger.account_sets.remove_member_in_op",
        skip(self, op, member),
        fields(account_set_id = %account_set_id),
        err(level = "warn")
    )]
    pub async fn remove_member_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        account_set_id: AccountSetId,
        member: impl Into<AccountSetMemberId>,
    ) -> Result<AccountSet, AccountSetError> {
        let member = member.into();

        let (account_set, member_id) = match member {
            AccountSetMemberId::Account(id) => {
                let set = self.repo.find_by_id_in_op(&mut *op, account_set_id).await?;
                (set, id)
            }
            AccountSetMemberId::AccountSet(id) => {
                let mut sets = self
                    .repo
                    .find_all_in_op::<AccountSet>(&mut *op, &[account_set_id, id])
                    .await?;
                let target = sets
                    .remove(&account_set_id)
                    .ok_or(AccountSetError::CouldNotFindById(account_set_id))?;
                let member_set = sets
                    .remove(&id)
                    .ok_or(AccountSetError::CouldNotFindById(id))?;

                if target.values().journal_id != member_set.values().journal_id {
                    return Err(AccountSetError::JournalIdMismatch);
                }

                (target, AccountId::from(id))
            }
        };

        self.assert_member_history_empty_in_op(
            op,
            account_set_id,
            account_set.values().journal_id,
            AccountId::from(&account_set.id()),
            member_id,
        )
        .await?;

        match member {
            AccountSetMemberId::Account(id) => {
                self.repo
                    .remove_member_account_and_return_parents(op, account_set_id, id)
                    .await?;
            }
            AccountSetMemberId::AccountSet(id) => {
                self.repo
                    .remove_member_set_and_return_parents(op, account_set_id, id)
                    .await?;
            }
        }

        Ok(account_set)
    }

    #[instrument(name = "cala_ledger.account_sets.find_all", skip(self))]
    pub async fn find_all<T: From<AccountSet>>(
        &self,
        account_set_ids: &[AccountSetId],
    ) -> Result<HashMap<AccountSetId, T>, AccountSetError> {
        Ok(self.repo.find_all(account_set_ids).await?)
    }

    #[instrument(name = "cala_ledger.account_sets.find_all_in_op", skip(self, op))]
    pub async fn find_all_in_op<T: From<AccountSet>>(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        account_set_ids: &[AccountSetId],
    ) -> Result<HashMap<AccountSetId, T>, AccountSetError> {
        Ok(self.repo.find_all_in_op(op, account_set_ids).await?)
    }

    #[instrument(name = "cala_ledger.account_sets.find", skip(self))]
    pub async fn find(&self, account_set_id: AccountSetId) -> Result<AccountSet, AccountSetError> {
        Ok(self.repo.find_by_id(account_set_id).await?)
    }

    #[instrument(name = "cala_ledger.account_sets.find_in_op", skip(self, op))]
    pub async fn find_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        account_set_id: AccountSetId,
    ) -> Result<AccountSet, AccountSetError> {
        Ok(self.repo.find_by_id_in_op(op, account_set_id).await?)
    }

    #[instrument(name = "cala_ledger.accounts_sets.find_by_external_id", skip(self))]
    pub async fn find_by_external_id(
        &self,
        external_id: String,
    ) -> Result<AccountSet, AccountSetError> {
        Ok(self.repo.find_by_external_id(Some(external_id)).await?)
    }

    #[instrument(name = "cala_ledger.account_sets.find_where_member", skip(self))]
    pub async fn find_where_member(
        &self,
        member: impl Into<AccountSetMemberId> + std::fmt::Debug,
        query: es_entity::PaginatedQueryArgs<AccountSetByNameCursor>,
    ) -> Result<es_entity::PaginatedQueryRet<AccountSet, AccountSetByNameCursor>, AccountSetError>
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
        args: es_entity::PaginatedQueryArgs<AccountSetByCreatedAtCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<AccountSet, AccountSetByCreatedAtCursor>,
        AccountSetError,
    > {
        Ok(self
            .repo
            .list_for_name_by_created_at(name, args, Default::default())
            .await?)
    }

    #[instrument(name = "cala_ledger.account_sets.list_for_name_in_op", skip(self, op))]
    pub async fn list_for_name_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        name: String,
        args: es_entity::PaginatedQueryArgs<AccountSetByCreatedAtCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<AccountSet, AccountSetByCreatedAtCursor>,
        AccountSetError,
    > {
        Ok(self
            .repo
            .list_for_name_by_created_at_in_op(op, name, args, Default::default())
            .await?)
    }

    #[instrument(
        name = "cala_ledger.account_sets.find_where_member_in_op",
        skip(self, op)
    )]
    pub async fn find_where_member_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        member: impl Into<AccountSetMemberId> + std::fmt::Debug,
        query: es_entity::PaginatedQueryArgs<AccountSetByNameCursor>,
    ) -> Result<es_entity::PaginatedQueryRet<AccountSet, AccountSetByNameCursor>, AccountSetError>
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
        args: es_entity::PaginatedQueryArgs<AccountSetMemberByCreatedAtCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<AccountSetMember, AccountSetMemberByCreatedAtCursor>,
        AccountSetError,
    > {
        self.repo.list_children_by_created_at(id, args).await
    }

    pub async fn list_members_by_created_at_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        id: AccountSetId,
        args: es_entity::PaginatedQueryArgs<AccountSetMemberByCreatedAtCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<AccountSetMember, AccountSetMemberByCreatedAtCursor>,
        AccountSetError,
    > {
        self.repo
            .list_children_by_created_at_in_op(op, id, args)
            .await
    }

    pub async fn list_members_by_external_id(
        &self,
        id: AccountSetId,
        args: es_entity::PaginatedQueryArgs<AccountSetMemberByExternalIdCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<
            AccountSetMemberByExternalId,
            AccountSetMemberByExternalIdCursor,
        >,
        AccountSetError,
    > {
        self.repo.list_children_by_external_id(id, args).await
    }

    pub async fn list_members_by_external_id_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        id: AccountSetId,
        args: es_entity::PaginatedQueryArgs<AccountSetMemberByExternalIdCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<
            AccountSetMemberByExternalId,
            AccountSetMemberByExternalIdCursor,
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
}

impl From<&AccountSetEvent> for OutboxEventPayload {
    fn from(event: &AccountSetEvent) -> Self {
        match event {
            AccountSetEvent::Initialized {
                values: account_set,
            } => OutboxEventPayload::AccountSetCreated {
                account_set: account_set.clone(),
            },
            AccountSetEvent::Updated { values, fields } => OutboxEventPayload::AccountSetUpdated {
                account_set: values.clone(),
                fields: fields.clone(),
            },
        }
    }
}
