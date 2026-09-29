mod helpers;

use cala_ledger::{
    account::{
        error::{AccountError, AccountRejection},
        Account,
    },
    *,
};

/// `AccountError` is a hand-rolled `{ Rejected, Transient, Fatal }` enum —
/// cala's public errors stay ordinary thiserror enums, not
/// `#[derive(errlanes::Failure)]` — so it does not implement
/// `errlanes::Laned` and `errlanes::retry` cannot take it directly. This is
/// exactly enough `Failure` surface (backed by a real, stored
/// `errlanes::Fail<AccountRejection>`, not a value rebuilt per call) to
/// drive `retry` over the public API below.
struct AccountFailure(errlanes::Fail<AccountRejection>);

impl std::fmt::Debug for AccountFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&self.0, f)
    }
}

impl std::fmt::Display for AccountFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0, f)
    }
}

impl std::error::Error for AccountFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        std::error::Error::source(&self.0)
    }
}

impl errlanes::Failure for AccountFailure {
    type Rejection = AccountRejection;

    fn into_fail(self) -> errlanes::Fail<AccountRejection> {
        self.0
    }

    fn from_fail(f: errlanes::Fail<AccountRejection>) -> Self {
        Self(f)
    }

    fn as_fail(&self) -> &errlanes::Fail<AccountRejection> {
        &self.0
    }
}

impl From<AccountError> for AccountFailure {
    fn from(e: AccountError) -> Self {
        Self(match e {
            AccountError::Rejected(r) => errlanes::Fail::Rejected(r),
            AccountError::Transient(t) => errlanes::Fail::Transient(t),
            AccountError::Fatal(f) => errlanes::Fail::Fatal(f),
        })
    }
}

#[tokio::test]
async fn find_returns_not_found_by_id() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config, &mut jobs).await?;

    let id = AccountId::new();
    match cala.accounts().find(id).await {
        Err(AccountError::Rejected(AccountRejection::NotFoundById(err_id))) => {
            assert_eq!(err_id, id)
        }
        Err(other) => panic!("expected NotFoundById({id}), got: {other}"),
        Ok(_) => panic!("expected not-found error, got Ok"),
    }

    Ok(())
}

/// An optimistic-concurrency conflict on an account update — two loaded
/// copies of the same account, the second write's expected event sequence
/// no longer matches — must classify as `Transient`, and `errlanes::retry`
/// must converge it. Mirrors the OCC conflict es-entity's own
/// `tests/snapshot_concurrency.rs` exercises, one layer up through cala's
/// hand-rolled `AccountError`.
#[tokio::test]
async fn occ_conflict_on_account_update_is_transient_and_retry_converges() -> anyhow::Result<()> {
    let pool = helpers::init_pool().await?;
    let mut jobs = helpers::init_jobs(pool.clone()).await?;
    let cala_config = CalaLedgerConfig::builder()
        .pool(pool)
        .exec_migrations(false)
        .build()?;
    let cala = CalaLedger::init(cala_config, &mut jobs).await?;

    // `update_status` is idempotent (`es_entity::Idempotent<()>`): toggling
    // to a copy's own current in-memory status is a no-op that stages no
    // event, which would make `persist` trivially succeed regardless of
    // staleness. Always toggle away from the copy's OWN in-memory value so
    // every persist below genuinely writes something.
    fn toggle(account: &mut Account) {
        let next = match account.values().status {
            Status::Active => Status::Locked,
            Status::Locked => Status::Active,
        };
        let _ = account.update_status(next);
    }

    let (new_account, _) = helpers::test_accounts();
    let account = cala.accounts().create(new_account).await?;
    let id = account.id();

    // Loaded before the account is mutated again below, so by the time it
    // tries to persist its in-memory event sequence is one behind.
    let mut stale = cala.accounts().find(id).await?;

    let mut fresh = cala.accounts().find(id).await?;
    toggle(&mut fresh);
    cala.accounts().persist(&mut fresh).await?;

    // Red first: the stale copy's write really does lose the CAS, and it
    // really does classify as Transient — not a simulated error.
    toggle(&mut stale);
    let direct_err = cala
        .accounts()
        .persist(&mut stale)
        .await
        .expect_err("a stale write must lose the optimistic-concurrency check");
    assert!(
        matches!(direct_err, AccountError::Transient(_)),
        "an OCC conflict must classify as Transient, got: {direct_err:?}"
    );

    // Now drive the same conflict through `errlanes::retry`: attempt 1
    // reuses a freshly-staled copy (guaranteed to fail again, for the same
    // reason as above), attempt 2 fetches fresh state and must succeed.
    let mut stale = cala.accounts().find(id).await?;
    let mut advance = cala.accounts().find(id).await?;
    toggle(&mut advance);
    cala.accounts().persist(&mut advance).await?;
    toggle(&mut stale);
    let mut first_attempt = Some(stale);
    let attempts = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let attempts_inner = attempts.clone();

    let outcome = errlanes::retry(&errlanes::RetryPolicy::default(), move || {
        attempts_inner.set(attempts_inner.get() + 1);
        let cala = cala.clone();
        let mut account: Option<Account> = first_attempt.take();
        async move {
            let mut account = match account.take() {
                Some(account) => account,
                None => cala
                    .accounts()
                    .find(id)
                    .await
                    .map_err(AccountFailure::from)?,
            };
            toggle(&mut account);
            cala.accounts()
                .persist(&mut account)
                .await
                .map_err(AccountFailure::from)
        }
    })
    .await;

    assert!(
        outcome.is_ok(),
        "retry must converge past the OCC conflict, got: {outcome:?}"
    );
    assert!(
        attempts.get() >= 2,
        "the first attempt must actually have hit the conflict, not succeeded outright: {} attempt(s)",
        attempts.get()
    );

    Ok(())
}
