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

#[cfg(test)]
mod tests {
    use super::*;
    use es_entity::errlanes::{Fail, FatalKind, ResultExt, TransientKind};
    use std::error::Error as _;

    #[test]
    fn database_faults_keep_their_lane_and_source_across_rejection_mapping() {
        let error: Fail<crate::account::error::AccountRejection, lanes!(Transient, Fatal)> =
            sqlx::Error::PoolTimedOut.into();
        let result = Err::<(), _>(error)
            .map_rejected(crate::account_set::error::AccountSetRejection::AccountRejection);
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

#[derive(Debug, errlanes::Classify)]
#[classify(fatal(Invariant), from)]
#[error("could not serialize a ledger value")]
pub(crate) struct CouldNotSerialize(#[source] serde_json::Error);
