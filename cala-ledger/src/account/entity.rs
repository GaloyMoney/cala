use es_entity::*;
use serde::{Deserialize, Serialize};

pub use cala_types::{account::*, primitives::AccountId, velocity::VelocityContextAccountValues};

use crate::primitives::*;

#[derive(EsEvent, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[serde(tag = "type", rename_all = "snake_case")]
#[es_event(id = "AccountId", event_context = false)]
pub enum AccountEvent {
    Initialized {
        values: AccountValues,
    },
    Updated {
        values: AccountValues,
        fields: Vec<String>,
    },
}

#[derive(EsEntity)]
pub struct Account {
    pub id: AccountId,
    values: AccountValues,
    events: EntityEvents<AccountEvent>,
}

impl Account {
    pub fn id(&self) -> AccountId {
        self.values.id
    }

    pub(super) fn is_account_set(&self) -> bool {
        self.values.config.is_account_set
    }

    pub fn values(&self) -> &AccountValues {
        &self.values
    }

    pub fn into_values(self) -> AccountValues {
        self.values
    }

    pub(super) fn context_values(&self) -> VelocityContextAccountValues {
        VelocityContextAccountValues::from(self.values())
    }

    pub fn update_status(&mut self, status: Status) -> es_entity::Idempotent<()> {
        self.update(AccountUpdate::builder().status(status).build())
    }

    pub fn update(&mut self, update: impl Into<AccountUpdate>) -> es_entity::Idempotent<()> {
        let AccountUpdate {
            external_id,
            code,
            name,
            normal_balance_type,
            description,
            status,
            metadata,
        } = update.into();

        let mut updated_fields = Vec::new();

        if let Some(code) = code {
            if code != self.values().code {
                self.values.code.clone_from(&code);
                updated_fields.push("code".to_string());
            }
        }
        if let Some(name) = name {
            if name != self.values().name {
                self.values.name.clone_from(&name);
                updated_fields.push("name".to_string());
            }
        }
        if let Some(normal_balance_type) = normal_balance_type {
            if normal_balance_type != self.values().normal_balance_type {
                self.values
                    .normal_balance_type
                    .clone_from(&normal_balance_type);
                updated_fields.push("normal_balance_type".to_string());
            }
        }
        if let Some(status) = status {
            if status != self.values().status {
                self.values.status.clone_from(&status);
                updated_fields.push("status".to_string());
            }
        }
        if external_id.is_some() && external_id != self.values().external_id {
            self.values.external_id.clone_from(&external_id);
            updated_fields.push("external_id".to_string());
        }
        if description.is_some() && description != self.values().description {
            self.values.description.clone_from(&description);
            updated_fields.push("description".to_string());
        }
        if let Some(metadata) = metadata {
            if metadata != serde_json::Value::Null
                && Some(&metadata) != self.values().metadata.as_ref()
            {
                self.values.metadata = Some(metadata);
                updated_fields.push("metadata".to_string());
            }
        }

        if updated_fields.is_empty() {
            return es_entity::Idempotent::AlreadyApplied;
        }

        self.events.push(AccountEvent::Updated {
            values: self.values.clone(),
            fields: updated_fields,
        });

        es_entity::Idempotent::Executed(())
    }

    pub fn created_at(&self) -> chrono::DateTime<chrono::Utc> {
        self.events
            .entity_first_persisted_at()
            .expect("Entity not persisted")
    }

    pub fn modified_at(&self) -> chrono::DateTime<chrono::Utc> {
        self.events
            .entity_last_modified_at()
            .expect("Entity not persisted")
    }

    pub fn metadata<T: serde::de::DeserializeOwned>(&self) -> Result<Option<T>, serde_json::Error> {
        match &self.values.metadata {
            Some(metadata) => Ok(Some(serde_json::from_value(metadata.clone())?)),
            None => Ok(None),
        }
    }
}

impl TryFromEvents<AccountEvent> for Account {
    fn try_from_events(events: EntityEvents<AccountEvent>) -> Result<Self, EntityHydrationError> {
        let mut hydrator = Account::hydrator();
        for event in events.iter_all() {
            match event {
                AccountEvent::Initialized { values } => {
                    hydrator.id(values.id).values(values.clone());
                }
                AccountEvent::Updated { values, .. } => {
                    hydrator.values(values.clone());
                }
            }
        }
        hydrator.build(events)
    }
}

#[derive(Debug, Default, bon::Builder)]
pub struct AccountUpdate {
    #[builder(into)]
    pub external_id: Option<String>,
    #[builder(into)]
    pub code: Option<String>,
    #[builder(into)]
    pub name: Option<String>,
    #[builder(into)]
    pub normal_balance_type: Option<DebitOrCredit>,
    #[builder(into)]
    pub description: Option<String>,
    #[builder(into)]
    pub status: Option<Status>,
    #[builder(with = |metadata: impl serde::Serialize| -> Result<_, serde_json::Error> {
        serde_json::to_value(metadata)
    })]
    pub metadata: Option<serde_json::Value>,
}

/// Representation of a ***new*** ledger account entity with required/optional properties and a builder.
#[derive(bon::Builder, Debug, Clone)]
pub struct NewAccount {
    #[builder(into)]
    pub id: AccountId,
    #[builder(into)]
    pub(super) code: String,
    #[builder(into)]
    pub(super) name: String,
    #[builder(into)]
    pub(super) external_id: Option<String>,
    #[builder(default)]
    pub(super) normal_balance_type: DebitOrCredit,
    #[builder(default)]
    pub(super) status: Status,
    /// Marks the account as eventually consistent: posters skip the
    /// exclusive per-(journal, account, currency) advisory lock and do not
    /// maintain its balance inline. The streaming EC rollup job
    /// (`cala.ec_balance_rollup`, registered in `CalaLedger::init`) is then
    /// the sole writer of its balance, folding the account's entries into
    /// `cala_current_balances` and `cala_balance_history` from the outbox
    /// stream.
    ///
    /// Sound for rollup-only counterparty accounts (e.g. product omnibus
    /// accounts that exist so double-entry postings share a counterparty),
    /// **including account-set members** — the rollup resolves set closure
    /// from live membership, not member history. Constraints:
    ///
    /// - **Attach before first entry.** A member may only join a set with
    ///   zero balance history (the attach guard rejects the rest, EC or
    ///   not): the rollup folds a member's activity from the point it
    ///   joins, so pre-join balance would be silently dropped.
    /// - **Readers must tolerate rollup lag.** The balance is eventually
    ///   consistent — do not read it in post-time enforcement paths.
    /// - **The rollup job must be running.** If it stalls, EC balances
    ///   drift silently behind the entry stream.
    #[builder(default)]
    pub(super) eventually_consistent: bool,
    #[builder(default, setters(vis = "pub(crate)"))]
    pub(super) is_account_set: bool,
    #[builder(into, setters(vis = "pub(crate)"))]
    velocity_context_values: Option<VelocityContextAccountValues>,
    #[builder(into)]
    description: Option<String>,
    #[builder(with = |metadata: impl serde::Serialize| -> Result<_, serde_json::Error> {
        serde_json::to_value(metadata)
    })]
    metadata: Option<serde_json::Value>,
    /// The account set this account joins **in the same atomic operation
    /// that creates it** — the lock-free create-inside-set fast path
    /// honored natively by `Accounts::create_in_op` /
    /// `Accounts::create_all_in_op`.
    ///
    /// `Option`, not a collection: **at most one set, enforced at the
    /// type level.** This is not a conservative v1 limit — k≥2 initial
    /// sets cannot be made lock-free at all (see below), so there is no
    /// value in a shape that pretends otherwise. Multiple sets at
    /// creation: plain create, then classic `AccountSets::add_member_in_op`
    /// calls.
    ///
    /// # Why the fast path needs (almost) none of the classic attach
    /// protocol
    ///
    /// For an account created and attached to exactly one set in the same
    /// atomic operation:
    ///
    /// 1. **MemberHasBalanceHistory is vacuously satisfied.** The account
    ///    row is uncommitted; `cala_entries` / `cala_balance_history`
    ///    rows FK `cala_accounts`, so no poster can have produced
    ///    activity for it. No `EXISTS` check and no class-1
    ///    (`EC_SET_LOCK_CLASS`) lock needed.
    /// 2. **The attach fence is moot.** The fence closes
    ///    attach-vs-in-flight-first-posting races. No posting to a
    ///    not-yet-committed account can be in flight; from the instant
    ///    the account becomes visible its membership already exists, so a
    ///    first posting resolves ancestors correctly.
    /// 3. **Account-level path uniqueness for k=1 memberships is implied
    ///    by set-level path uniqueness.** With exactly one direct
    ///    membership in set S, two account→ancestor paths would require
    ///    two S→ancestor paths — which is precisely the set-level
    ///    path-uniqueness invariant that every set-structure mutation
    ///    (`AccountSets::add_member` with a set member /
    ///    `add_member_sets`) enforces under the coarse EXCLUSIVE lock. That validation reads only
    ///    set→set edges; a concurrent fast-path member row (committed or
    ///    not) is irrelevant to it. Therefore no interleaving with a
    ///    concurrent structure op can create a double-path for a
    ///    single-membership account, and the fast path needs **no coarse
    ///    SHARED lock, no graph read, and no
    ///    `assert_no_double_membership` check**.
    /// 4. **Same-account race guard.** A concurrent *classic*
    ///    `add_member(s)` of the same fresh account (possible only if the
    ///    caller leaks the client-generated id before commit) is fenced
    ///    by retaining the **class-2 per-member EXCLUSIVE**
    ///    (`MEMBER_LOCK_CLASS = 2`, `crate::account_set_member`) in the
    ///    fast path: the classic attacher blocks on it and, after our
    ///    commit, its probe sees membership {S}. This is the ONLY
    ///    advisory lock the fast path takes; it is uncontended in sane
    ///    usage. Lock-ordering safety: the fast path never waits on the
    ///    coarse lock while holding class-2 (it never touches the coarse
    ///    lock at all), so the doctrine in the `ADDVISORY_LOCK_ID` doc
    ///    comment (`account_set/repo.rs`) is not violated. **Composition
    ///    rule:** within one op, sequence classic membership mutations
    ///    BEFORE fast-path creates, or don't mix the two — a fast-path
    ///    create followed by a classic add in the same op inverts this
    ///    ordering (holds class-2, then waits on coarse), which can
    ///    deadlock (detected, not silent) against a concurrent classic
    ///    attacher of the same fresh account.
    /// 5. **EC semantics.** Membership exists from account birth; the
    ///    streaming rollup folds from birth. Strictly safer than
    ///    create-then-attach.
    ///
    /// # Why k≥2 initial sets cannot be made lock-free
    ///
    /// k=1 needs no graph read because set-level path uniqueness already
    /// implies account-level uniqueness for a single membership (point 3
    /// above). k=2 has no such implication: set-level uniqueness allows
    /// `S1 → A` and `S2 → A` simultaneously (a perfectly legal graph), so
    /// an account joining both S1 and S2 could reach A twice — the
    /// prohibited double-membership state — and detecting that requires
    /// READING the ancestor closures of S1 and S2 to check they're
    /// disjoint. That read is irreparably racy without the coarse lock:
    /// a concurrent structure op adding `S2 → A` validates against
    /// COMMITTED member rows only, and our uncommitted edges are
    /// invisible to it — both transactions validate cleanly, both
    /// commit, and the account ends up with two paths to A. The only
    /// fences that close this are coarse SHARED (i.e. the classic
    /// protocol) or pre-commit visibility (impossible). There is no
    /// lock-free ordering that closes it, so k=1 is not a starting
    /// point to be relaxed later — it is the boundary of this fast
    /// path's invariant.
    ///
    /// Accepted caveat: the fast path *relies on* the set-level
    /// uniqueness invariant instead of re-verifying account-level
    /// uniqueness per call. A pre-existing corrupted set graph (a
    /// diamond above S) would double-count the new account without
    /// being detected at attach time; the classic path's path-count
    /// would have caught it. The invariant is re-verified on every
    /// structure mutation and the soak drift-check remains the backstop.
    ///
    /// # Transient — MUST stay out of persisted state
    ///
    /// This field is intentionally absent from `AccountValues`,
    /// `into_values()`, and `IntoEvents`: membership's single source of
    /// truth is the member table + `AccountSetMemberCreated` outbox
    /// events, and duplicating the set id into the account's event
    /// stream would go stale on any later `remove_member`.
    #[builder(into)]
    pub(super) initial_account_set: Option<AccountSetId>,
}

impl NewAccount {
    pub(super) fn context_values(&self) -> VelocityContextAccountValues {
        self.velocity_context_values
            .clone()
            .unwrap_or_else(|| VelocityContextAccountValues::from(self.clone().into_values()))
    }

    pub(super) fn into_values(self) -> AccountValues {
        AccountValues {
            id: self.id,
            version: 1,
            code: self.code,
            name: self.name,
            external_id: self.external_id,
            normal_balance_type: self.normal_balance_type,
            status: self.status,
            description: self.description,
            metadata: self.metadata,
            config: AccountConfig {
                is_account_set: self.is_account_set,
                eventually_consistent: self.eventually_consistent,
            },
        }
    }
}

impl IntoEvents<AccountEvent> for NewAccount {
    type IdSource = FromCaller;

    fn into_events(
        self,
        builder: EntityEventsBuilder<AccountEvent, FromCaller>,
    ) -> EntityEvents<AccountEvent> {
        let values = self.into_values();
        builder.initial_events(values.id, [AccountEvent::Initialized { values }])
    }
}

impl<S: new_account_builder::State> NewAccountBuilder<S> {
    /// Choose how this account's balance is maintained.
    ///
    /// - [`BalanceRollup::Synchronous`] (the default): the balance is
    ///   written inline in every posting transaction under the poster lock,
    ///   so it is immediately readable (read-your-write).
    /// - [`BalanceRollup::EventuallyConsistent`]: posting takes no balance
    ///   lock and writes no balance inline; the balance is maintained
    ///   asynchronously by the streaming rollup job. `Balances::find`
    ///   therefore *lags* until the rollup catches up. Only pick this for
    ///   accounts no flow reads synchronously right after posting (e.g.
    ///   product omnibus summary accounts).
    pub fn balance_rollup(
        self,
        balance_rollup: BalanceRollup,
    ) -> NewAccountBuilder<new_account_builder::SetEventuallyConsistent<S>>
    where
        S::EventuallyConsistent: new_account_builder::IsUnset,
    {
        self.eventually_consistent(matches!(
            balance_rollup,
            BalanceRollup::EventuallyConsistent
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_builds() {
        let new_account = NewAccount::builder()
            .id(uuid::Uuid::now_v7())
            .code("code")
            .name("name")
            .build();
        assert_eq!(new_account.code, "code");
        assert_eq!(new_account.name, "name");
        assert_eq!(new_account.normal_balance_type, DebitOrCredit::Credit);
        assert_eq!(new_account.description, None);
        assert_eq!(new_account.status, Status::Active);
        assert_eq!(new_account.metadata, None);
    }

    #[test]
    fn defaults_to_synchronous_balance_rollup() {
        let new_account = NewAccount::builder()
            .id(uuid::Uuid::now_v7())
            .code("code")
            .name("name")
            .build();
        assert!(!new_account.eventually_consistent);
    }

    #[test]
    fn balance_rollup_setter_toggles_eventually_consistent() {
        let ec = NewAccount::builder()
            .id(uuid::Uuid::now_v7())
            .code("ec-code")
            .name("name")
            .balance_rollup(BalanceRollup::EventuallyConsistent)
            .build();
        assert!(ec.eventually_consistent);

        let sync = NewAccount::builder()
            .id(uuid::Uuid::now_v7())
            .code("sync-code")
            .name("name")
            .balance_rollup(BalanceRollup::Synchronous)
            .build();
        assert!(!sync.eventually_consistent);
    }

    #[test]
    fn accepts_metadata() {
        use serde_json::json;
        let new_account = NewAccount::builder()
            .id(uuid::Uuid::now_v7())
            .code("code")
            .name("name")
            .metadata(json!({"foo": "bar"}))
            .unwrap()
            .build();
        assert_eq!(new_account.metadata, Some(json!({"foo": "bar"})));
    }
}
