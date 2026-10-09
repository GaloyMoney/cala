use es_entity::errlanes;

/// The EC rollup did not reach the requested fence before the caller's deadline.
/// Cala owns a registered singleton; a missing handle is an invariant fault.
#[derive(Debug, errlanes::Rejection, errlanes::Lift)]
#[rejection(code = "CALA_EC_CAUGHT_UP_TIMEOUT")]
#[error("EC rollup checkpoint {applied} had not reached {frontier} after {waited:?}")]
#[lift(
    obix::out::SubscriptionRejection,
    variant = CaughtUpTimeout,
    unhandled = fatal
)]
pub struct EcCaughtUpTimeout {
    #[lift(from = checkpoint)]
    pub applied: obix::StreamPosition,
    #[lift(from = target)]
    pub frontier: obix::StreamPosition,
    pub waited: std::time::Duration,
}

#[cfg(test)]
mod tests {
    use super::*;
    use errlanes::{lanes, Fail, FatalKind, ResultExt};
    use obix::out::SubscriptionRejection;

    #[test]
    fn only_the_deadline_is_actionable_for_a_registered_rollup() {
        let waited = std::time::Duration::from_millis(7);
        let source = SubscriptionRejection::CaughtUpTimeout {
            checkpoint: obix::StreamPosition::Insert(obix::EventSequence::from(3)),
            target: obix::StreamPosition::Insert(obix::EventSequence::from(9)),
            waited,
        };
        let result: Result<(), Fail<EcCaughtUpTimeout, lanes!(Transient, Fatal)>> = Err(source)
            .lift::<EcCaughtUpTimeout>()
            .map_err(|e| e.lift::<EcCaughtUpTimeout, lanes!(Transient, Fatal)>());
        let Fail::Rejected(timeout) = result.unwrap_err() else {
            panic!("timeout")
        };
        assert_eq!(timeout.waited, waited);
        assert_eq!(
            timeout.applied,
            obix::StreamPosition::Insert(obix::EventSequence::from(3))
        );
        assert_eq!(
            timeout.frontier,
            obix::StreamPosition::Insert(obix::EventSequence::from(9))
        );

        let source = SubscriptionRejection::NoSuchJob {
            subscriber_type: "rollup".into(),
            key: "private-key".into(),
        };
        let result: Result<(), Fail<EcCaughtUpTimeout, lanes!(Transient, Fatal)>> = Err(source)
            .lift::<EcCaughtUpTimeout>()
            .map_err(|e| e.lift::<EcCaughtUpTimeout, lanes!(Transient, Fatal)>());
        let Fail::Fatal(fatal) = result.unwrap_err() else {
            panic!("invariant")
        };
        assert_eq!(fatal.kind, FatalKind::Invariant);
        assert!(!fatal.to_string().contains("private-key"));
    }
}
