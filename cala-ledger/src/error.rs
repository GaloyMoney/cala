//! Cala classifies failures at their origin. Caller-correctable outcomes are
//! typed rejections; infrastructure and invariant failures use `CalaFault`.
//! Public rejecting signatures spell `Fail<Rejection, lanes!(Transient, Fatal)>`.
//! Stored decoding failures retain their sources as `Fatal(CorruptState)`.
use es_entity::errlanes::{self, lanes, Fault};

pub type CalaFault = Fault<lanes!(Transient, Fatal)>;

#[derive(Debug, errlanes::Classify)]
#[classify(fatal(Config), from)]
#[error("ledger migration failed")]
pub(crate) struct Migrate(#[source] sqlx::migrate::MigrateError);

#[derive(Debug, errlanes::Classify)]
#[classify(fatal(CorruptState), from)]
#[error("could not decode stored ledger data")]
pub(crate) struct CouldNotDecodeStored(#[source] serde_json::Error);

#[derive(Debug, errlanes::Classify)]
#[classify(fatal(CorruptState), from)]
#[error("could not decode stored ledger currency")]
pub(crate) struct CouldNotDecodeCurrency(#[source] crate::primitives::ParseCurrencyError);

#[derive(Debug, errlanes::Classify)]
#[classify(fatal(Invariant), from)]
#[error("could not serialize a ledger value")]
pub(crate) struct CouldNotSerialize(#[source] serde_json::Error);

#[cfg(test)]
mod tests {
    use super::*;
    use es_entity::errlanes::{lanes, Fail, FatalKind, ResultExt, TransientKind};
    use std::error::Error as _;

    #[test]
    fn database_faults_keep_their_lane_and_source_across_widening() {
        let error: Fail<crate::posting::PostingRejection, lanes!(Transient, Fatal)> =
            sqlx::Error::PoolTimedOut.into();
        let result = Err::<(), _>(error)
            .widen::<Fail<crate::posting::BatchPostingRejection, lanes!(Transient, Fatal)>>();
        let Fail::Transient(transient) = result.unwrap_err() else {
            panic!("transient")
        };
        assert_eq!(transient.kind, TransientKind::PoolTimeout);
        assert!(transient.source().unwrap().is::<sqlx::Error>());

        let error: CalaFault = sqlx::Error::Protocol("broken protocol".into()).into();
        let Fault::Fatal(fatal) = error else {
            panic!("fatal")
        };
        assert_eq!(fatal.kind, FatalKind::Dependency);
        assert!(fatal.source().unwrap().is::<sqlx::Error>());
    }

    #[test]
    fn stored_decode_overrides_serde_and_survives_the_job_boundary() {
        let result = serde_json::from_value::<u64>(serde_json::json!("bad"))
            .classify::<CouldNotDecodeStored>()
            .widen::<CalaFault>();
        let boxed: Box<dyn std::error::Error + Send + Sync> = Box::new(result.unwrap_err());
        let Fault::Fatal(fatal) = Fault::classify(&*boxed).narrow_denied() else {
            panic!("fatal")
        };
        assert_eq!(fatal.kind, FatalKind::CorruptState);
        let wrapper = fatal.source().expect("decode wrapper");
        assert!(wrapper.is::<CouldNotDecodeStored>());
        assert!(wrapper.source().unwrap().is::<serde_json::Error>());
    }

    #[test]
    fn migration_failure_is_configuration_and_keeps_the_driver_source() {
        let error: CalaFault = Migrate(sqlx::migrate::MigrateError::VersionMissing(42)).into();
        let Fault::Fatal(fatal) = error else {
            panic!("fatal")
        };
        assert_eq!(fatal.kind, FatalKind::Config);
        assert!(fatal
            .source()
            .unwrap()
            .source()
            .unwrap()
            .is::<sqlx::migrate::MigrateError>());
    }
}

#[cfg(test)]
mod sql_contract_tests {
    use es_entity::errlanes::{lanes, Fail, FatalKind, ResultExt, TransientKind};
    use std::error::Error;

    // Real PostgreSQL diagnostics exercise SQLSTATE and constraint extraction,
    // including the unknown-constraint native-classification path.
    async fn violation(name: &str, code: &str) -> sqlx::Error {
        let pool = sqlx::PgPool::connect(&std::env::var("PG_CON").unwrap())
            .await
            .unwrap();
        let statement = format!("DO $$ BEGIN RAISE EXCEPTION 'contract fixture' USING ERRCODE = '{code}', CONSTRAINT = '{name}'; END $$");
        let error = sqlx::query(&statement).execute(&pool).await.unwrap_err();
        pool.close().await;
        error
    }

    #[tokio::test]
    async fn posting_constraints_reject_and_unknown_sql_keeps_its_lane_and_source() {
        use crate::posting::error::{ApplyPostingRejection, PostWrite, PostingRejection};
        for (constraint, code, expected) in [
            ("cala_transactions_pkey", "23505", "id"),
            ("cala_transactions_external_id_key", "23505", "external"),
            (
                "cala_entries_account_not_account_set_fkey",
                "23503",
                "entry",
            ),
        ] {
            let result = Err::<(), _>(violation(constraint, code).await)
                .classify::<PostWrite>()
                .widen::<Fail<PostingRejection, lanes!(Transient, Fatal)>>();
            let actual = match result.unwrap_err().rejected().unwrap() {
                PostingRejection::Apply(ApplyPostingRejection::DuplicateTransactionId) => "id",
                PostingRejection::Apply(ApplyPostingRejection::DuplicateExternalId) => "external",
                PostingRejection::Apply(ApplyPostingRejection::EntryTargetsAccountSet) => "entry",
                other => panic!("unexpected posting rejection: {other:?}"),
            };
            assert_eq!(actual, expected);
        }
        let error = Err::<(), _>(violation("unrecognized_constraint", "23505").await)
            .classify::<PostWrite>()
            .widen::<Fail<PostingRejection, lanes!(Transient, Fatal)>>()
            .unwrap_err();
        let Fail::Fatal(fault) = error else {
            panic!("unknown constraint must retain native fault classification")
        };
        assert_eq!(fault.kind, FatalKind::Invariant);
        assert!(fault.source().unwrap().is::<sqlx::Error>());
        let error = Err::<(), _>(sqlx::Error::PoolTimedOut)
            .classify::<PostWrite>()
            .widen::<Fail<PostingRejection, lanes!(Transient, Fatal)>>()
            .unwrap_err();
        let Fail::Transient(fault) = error else {
            panic!("pool timeout")
        };
        assert_eq!(fault.kind, TransientKind::PoolTimeout);
        assert!(fault.source().unwrap().is::<sqlx::Error>());
    }

    #[tokio::test]
    async fn membership_and_limit_attachment_recognize_only_their_constraints() {
        use crate::{
            account_set::error::{MemberAlreadyAdded, MembershipWrite},
            velocity::error::{AttachLimit, LimitAlreadyAddedToControl},
        };
        for name in [
            "cala_account_set_member_accou_account_set_id_member_account_key",
            "cala_account_set_member_accou_account_set_id_member_accoun_key1",
        ] {
            assert!(matches!(
                Err::<(), _>(violation(name, "23505").await)
                    .classify::<MembershipWrite>()
                    .widen::<Fail<MemberAlreadyAdded, lanes!(Transient, Fatal)>>(),
                Err(Fail::Rejected(MemberAlreadyAdded))
            ));
        }
        assert!(matches!(
            Err::<(), _>(
                violation(
                    "cala_velocity_control_limits_velocity_control_id_velocity_l_key",
                    "23505"
                )
                .await
            )
            .classify::<AttachLimit>()
            .widen::<Fail<LimitAlreadyAddedToControl, lanes!(Transient, Fatal)>>(),
            Err(Fail::Rejected(LimitAlreadyAddedToControl))
        ));
        assert!(matches!(
            Err::<(), _>(violation("unknown", "23505").await)
                .classify::<MembershipWrite>()
                .widen::<Fail<MemberAlreadyAdded, lanes!(Transient, Fatal)>>(),
            Err(Fail::Fatal(_))
        ));
        assert!(matches!(
            Err::<(), _>(violation("unknown", "23505").await)
                .classify::<AttachLimit>()
                .widen::<Fail<LimitAlreadyAddedToControl, lanes!(Transient, Fatal)>>(),
            Err(Fail::Fatal(_))
        ));
    }
}

#[cfg(test)]
mod rejection_code_contracts {
    use crate::param::ParamDefaultRejectionCode;
    use crate::{
        account::error::*, account_set::error::*, balance::error::*, journal::error::*,
        ledger::error::*, posting::error::*, transaction::error::*, tx_template::error::*,
        velocity::error::*,
    };
    use cala_types::{param::*, primitives::*};
    use cel_interpreter::*;

    #[test]
    fn shared_conflict_lifts_preserve_attempted_values() {
        use es_entity::errlanes::{lanes, Fail, ResultExt};
        use std::error::Error;

        macro_rules! check {
            ($source:path, $leaf:ty, [$($owner:ty),+], [$($attempted:expr),+]) => {
                for attempted in [$($attempted),+] {
                    $(
                        let conflict = es_entity::ConstraintConflict::new(
                            attempted.clone(),
                            "fixture", "fixture_key", es_entity::ConstraintKind::Unique,
                            sqlx::Error::Protocol("constraint fixture".into()),
                        );
                        let result: Result<(), Fail<$owner, lanes!(Fatal)>> =
                            Err::<(), _>($source(conflict)).widen();
                        let Fail::Rejected(rejection) = result.unwrap_err() else {
                            panic!("known constraint must reject");
                        };
                        let leaf = rejection.source().unwrap().downcast_ref::<$leaf>().unwrap();
                        assert_eq!(leaf.0, attempted);
                    )+
                }
            };
        }
        check!(
            AccountConstraintViolation::CodeKey,
            AccountCodeAlreadyExists,
            [CreateAccountRejection, PersistAccountRejection],
            [None, Some("code".to_owned())]
        );
        check!(
            AccountConstraintViolation::ExternalIdKey,
            AccountExternalIdAlreadyExists,
            [CreateAccountRejection, PersistAccountRejection],
            [None, Some(None), Some(Some("external".to_owned()))]
        );
        check!(
            JournalConstraintViolation::CodeKey,
            JournalCodeAlreadyExists,
            [CreateJournalRejection, PersistJournalRejection],
            [None, Some(None), Some(Some("code".to_owned()))]
        );
        check!(
            AccountSetConstraintViolation::ExternalIdKey,
            AccountSetExternalIdAlreadyExists,
            [CreateAccountSetRejection, PersistAccountSetRejection],
            [None, Some(None), Some(Some("external".to_owned()))]
        );
    }

    #[test]
    fn rendering_codes_have_one_canonical_declaration() {
        // ALL lists locally owned codes, excluding delegated/forwarded codes.
        // Register every source family as well as composed contracts so that
        // reuse through composition and shared leaves retains one code owner.
        macro_rules! catalogs {
            ($($code:ident),+ $(,)?) => { [$( (stringify!($code), $code::ALL) ),+] };
        }
        let catalogs = catalogs![
            CelParseRejectionCode,
            CelTypeMismatchCode,
            CoreTypeCoercionCode,
            ExternalTypeCoercionCode,
            JsonCoercionRejectionCode,
            CelConversionRejectionCode,
            ExternalParseErrorCode,
            ParamValueRejectionCode,
            ParamDefaultRejectionCode,
            UnsupportedParamTypeCode,
            ParseLayerErrorCode,
            ParseCurrencyErrorCode,
            AccountCodeAlreadyExistsCode,
            AccountExternalIdAlreadyExistsCode,
            JournalCodeAlreadyExistsCode,
            AccountSetExternalIdAlreadyExistsCode,
            AccountSetJournalIdMismatchCode,
            PersistAccountRejectionCode,
            AccountNotFoundCode,
            AccountCodeNotFoundCode,
            AccountExternalIdNotFoundCode,
            InitialAccountSetNotFoundCode,
            CreateAccountRejectionCode,
            SetAccountStatusRejectionCode,
            AccountSetNotFoundCode,
            AccountSetExternalIdNotFoundCode,
            PersistAccountSetRejectionCode,
            CreateAccountSetRejectionCode,
            MemberAlreadyAddedCode,
            MemberHasBalanceHistoryCode,
            AddAccountMembersRejectionCode,
            AddSetMembersRejectionCode,
            AddMemberRejectionCode,
            RemoveMemberRejectionCode,
            BalanceNotFoundCode,
            BalanceAccountLockedCode,
            CreateJournalRejectionCode,
            PersistJournalRejectionCode,
            JournalNotFoundCode,
            JournalCodeNotFoundCode,
            TransactionNotFoundCode,
            TransactionExternalIdNotFoundCode,
            CreateTxTemplateRejectionCode,
            TxTemplateNotFoundCode,
            EcCaughtUpTimeoutCode,
            TooManyPostingBalancesCode,
            PostingRejectionCode,
            PreparePostingRejectionCode,
            ValidatePostingRejectionCode,
            ApplyPostingRejectionCode,
            BatchPreparePostingRejectionCode,
            BatchPostingRejectionCode,
            LimitExceededErrorCode,
            EnforceVelocityRejectionCode,
            CreateVelocityControlRejectionCode,
            CreateVelocityLimitRejectionCode,
            LimitAlreadyAddedToControlCode,
            AttachVelocityControlRejectionCode,
        ];
        let mut owners = std::collections::HashMap::new();
        for (owner, codes) in catalogs {
            for code in codes {
                assert!(!code.is_empty());
                assert!(code
                    .bytes()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_'));
                assert!(
                    owners.insert(code, owner).is_none(),
                    "duplicate rendering code {code} in {owner}"
                );
            }
        }
    }
}
