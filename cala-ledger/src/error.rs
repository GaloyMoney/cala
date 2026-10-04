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
    fn database_faults_keep_their_lane_and_source_across_rejection_mapping() {
        let error: Fail<
            crate::account::error::CreateBackingAccountRejection,
            lanes!(Transient, Fatal),
        > = sqlx::Error::PoolTimedOut.into();
        let result = Err::<(), _>(error)
            .map_rejected(crate::account_set::error::CreateAccountSetRejection::from);
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
        use crate::posting::error::{PostWrite, PostWriteRejection};
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
                .widen::<Fail<PostWriteRejection, lanes!(Transient, Fatal)>>();
            let actual = match result.unwrap_err().rejected().unwrap() {
                PostWriteRejection::DuplicateTransactionId => "id",
                PostWriteRejection::DuplicateExternalId => "external",
                PostWriteRejection::EntryTargetsAccountSet => "entry",
            };
            assert_eq!(actual, expected);
        }
        let error = Err::<(), _>(violation("unrecognized_constraint", "23505").await)
            .classify::<PostWrite>()
            .widen::<Fail<PostWriteRejection, lanes!(Transient, Fatal)>>()
            .unwrap_err();
        let Fail::Fatal(fault) = error else {
            panic!("unknown constraint must retain native fault classification")
        };
        assert_eq!(fault.kind, FatalKind::Invariant);
        assert!(fault.source().unwrap().is::<sqlx::Error>());
        let error = Err::<(), _>(sqlx::Error::PoolTimedOut)
            .classify::<PostWrite>()
            .widen::<Fail<PostWriteRejection, lanes!(Transient, Fatal)>>()
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
