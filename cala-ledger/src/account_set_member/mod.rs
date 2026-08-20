//! The account-member edge (`cala_account_set_member_accounts`): its own
//! module, standing between [`crate::account`] and [`crate::account_set`]
//! rather than inside either of them.
//!
//! ```text
//! Accounts ─────▶ AccountSetMembers ─────▶ AccountSetMemberRepo
//! AccountSets ──▶ AccountSetMembers
//! AccountSets ──▶ Accounts                (unchanged — backing-account create)
//! ```
//!
//! This module owns every write to the edge table (the class-2 per-member
//! advisory lock, the insert, the delete) AND its public list reads — full
//! ownership, not just the write primitive. It has two callers with
//! different lock protocols:
//!
//! - the **classic attach/detach protocol** (`account_set` module):
//!   arbitrary accounts, fenced by coarse SHARED (`AccountSetRepo`) +
//!   per-member EXCLUSIVE (this module) + the balance-history guard +
//!   path-uniqueness validation, all sequenced by the `AccountSets`
//!   service;
//! - the **create-inside-set fast path** (`account` module,
//!   `NewAccount::initial_account_set`): a freshly created account joining
//!   exactly one set in the same atomic operation, fenced by the
//!   per-member EXCLUSIVE alone (the invariant argument lives on
//!   `NewAccount::initial_account_set`'s field docs).
//!
//! Crate-internal leaf w.r.t. the entity modules: this module imports only
//! `cala_types`, `es_entity`, `sqlx`, and the outbox — never
//! `crate::account` or `crate::account_set` — keeping the module graph a
//! DAG. It is not itself an `EsRepo`/entity: the edge is a plain relation
//! plus an outbox event, not event-sourced.
mod error;
mod repo;

use sqlx::PgPool;

use crate::{
    outbox::OutboxPublisher,
    primitives::{AccountId, AccountSetId},
};

pub(crate) use error::AccountSetMemberError;
pub use repo::members_cursor;
use repo::AccountSetMemberRepo;

#[derive(Clone)]
pub(crate) struct AccountSetMembers {
    repo: AccountSetMemberRepo,
}

impl AccountSetMembers {
    pub(crate) fn new(pool: &PgPool, publisher: &OutboxPublisher) -> Self {
        Self {
            repo: AccountSetMemberRepo::new(pool, publisher),
        }
    }

    pub(crate) async fn lock_members_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        account_ids: &[AccountId],
    ) -> Result<(), sqlx::Error> {
        self.repo.lock_members_in_op(db, account_ids).await
    }

    pub(crate) async fn add_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        pairs: &[(AccountSetId, AccountId)],
    ) -> Result<(), sqlx::Error> {
        self.repo.add_in_op(db, pairs).await
    }

    pub(crate) async fn attach_new_accounts_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        pairs: &[(AccountSetId, AccountId)],
    ) -> Result<(), AccountSetMemberError> {
        self.repo.attach_new_accounts_in_op(db, pairs).await
    }

    pub(crate) async fn remove_in_op(
        &self,
        db: &mut impl es_entity::AtomicOperation,
        account_set_id: AccountSetId,
        account_id: AccountId,
    ) -> Result<(), sqlx::Error> {
        self.repo.remove_in_op(db, account_set_id, account_id).await
    }

    pub(crate) async fn list_by_created_at(
        &self,
        id: AccountSetId,
        args: es_entity::PaginatedQueryArgs<members_cursor::AccountSetMemberByCreatedAtCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<
            cala_types::account_set::AccountSetMember,
            members_cursor::AccountSetMemberByCreatedAtCursor,
        >,
        sqlx::Error,
    > {
        self.repo.list_by_created_at(id, args).await
    }

    pub(crate) async fn list_by_created_at_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        id: AccountSetId,
        args: es_entity::PaginatedQueryArgs<members_cursor::AccountSetMemberByCreatedAtCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<
            cala_types::account_set::AccountSetMember,
            members_cursor::AccountSetMemberByCreatedAtCursor,
        >,
        sqlx::Error,
    > {
        self.repo.list_by_created_at_in_op(op, id, args).await
    }

    pub(crate) async fn list_by_external_id(
        &self,
        id: AccountSetId,
        args: es_entity::PaginatedQueryArgs<members_cursor::AccountSetMemberByExternalIdCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<
            cala_types::account_set::AccountSetMemberByExternalId,
            members_cursor::AccountSetMemberByExternalIdCursor,
        >,
        sqlx::Error,
    > {
        self.repo.list_by_external_id(id, args).await
    }

    pub(crate) async fn list_by_external_id_in_op(
        &self,
        op: &mut impl es_entity::AtomicOperation,
        id: AccountSetId,
        args: es_entity::PaginatedQueryArgs<members_cursor::AccountSetMemberByExternalIdCursor>,
    ) -> Result<
        es_entity::PaginatedQueryRet<
            cala_types::account_set::AccountSetMemberByExternalId,
            members_cursor::AccountSetMemberByExternalIdCursor,
        >,
        sqlx::Error,
    > {
        self.repo.list_by_external_id_in_op(op, id, args).await
    }
}
