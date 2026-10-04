use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use cala_ledger::{
    account::error::AccountExternalIdNotFound,
    errlanes::{self, lanes, Fail},
};
use tracing::{
    field::{Field, Visit},
    span::{Attributes, Id, Record},
    Subscriber,
};
use tracing_subscriber::{layer::Context, prelude::*, Layer};

type Fields = Arc<Mutex<HashMap<String, String>>>;
#[derive(Clone)]
struct Capture(Fields);
struct Visitor<'a>(&'a mut HashMap<String, String>);
impl Visit for Visitor<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.0.insert(field.name().into(), format!("{value:?}"));
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.insert(field.name().into(), value.into());
    }
}
impl<S: Subscriber> Layer<S> for Capture {
    fn on_new_span(&self, attrs: &Attributes<'_>, _: &Id, _: Context<'_, S>) {
        attrs.record(&mut Visitor(&mut self.0.lock().unwrap()));
    }
    fn on_record(&self, _: &Id, record: &Record<'_>, _: Context<'_, S>) {
        record.record(&mut Visitor(&mut self.0.lock().unwrap()));
    }
}

#[errlanes::instrument(skip_all)]
fn boundary(
    error: Fail<AccountExternalIdNotFound, lanes!(Transient, Fatal)>,
) -> Result<(), Fail<AccountExternalIdNotFound, lanes!(Transient, Fatal)>> {
    Err(error)
}

// Separate test binary: tracing's callsite-interest cache is process-wide.
#[test]
fn lane_fields_distinguish_rejections_from_faults_without_rendering_caller_input() {
    let fields = Fields::default();
    let subscriber = tracing_subscriber::registry().with(Capture(fields.clone()));
    tracing::subscriber::with_default(subscriber, || {
        let _ = boundary(AccountExternalIdNotFound("private-caller-input".into()).into());
        {
            let recorded = fields.lock().unwrap();
            assert_eq!(
                recorded.get("error.lane").map(String::as_str),
                Some("rejected")
            );
            assert_eq!(
                recorded.get("error.code").map(String::as_str),
                Some("CALA_ACCOUNT_COULD_NOT_FIND_BY_EXTERNAL_ID")
            );
            assert!(!format!("{recorded:?}").contains("private-caller-input"));
        }
        fields.lock().unwrap().clear();
        let _ = boundary(sqlx::Error::PoolTimedOut.into());
        let recorded = fields.lock().unwrap();
        assert_eq!(
            recorded.get("error.lane").map(String::as_str),
            Some("transient")
        );
        assert!(recorded.contains_key("exception.message"));
    });
}
