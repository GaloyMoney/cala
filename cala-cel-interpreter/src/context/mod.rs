use std::borrow::Cow;

use cel::Context;
use es_entity::clock::{Clock, ClockHandle};

use crate::{builtins, value::CelValue};

pub struct CelContext {
    inner: Context<'static>,
    clock: ClockHandle,
    debug_vars: Vec<(String, CelValue)>,
}

impl CelContext {
    pub fn add_variable(&mut self, name: impl Into<Cow<'static, str>>, value: impl Into<CelValue>) {
        let name = name.into();
        let name_string = name.to_string();
        let value = value.into();
        self.inner
            .add_variable_from_value(name_string.clone(), value.clone().into_cel_value());
        self.debug_vars.push((name_string, value));
    }

    pub(crate) fn inner(&self) -> &Context<'static> {
        &self.inner
    }

    pub fn debug_context(&self) -> String {
        if self.debug_vars.is_empty() {
            String::new()
        } else {
            self.debug_vars
                .iter()
                .map(|(name, value)| format!("{name}={value:?}"))
                .collect::<Vec<_>>()
                .join(", ")
        }
    }

    pub fn new() -> Self {
        Self::new_with_clock(Clock::handle().clone())
    }

    pub fn new_with_clock(clock: ClockHandle) -> Self {
        let mut inner = Context::default();

        let date_clock = clock.clone();
        inner.add_function("date", move |args| builtins::date(date_clock.clone(), args));
        inner.add_function("uuid", builtins::uuid);
        inner.add_function("decimal", builtins::decimal);
        inner.add_function("decimal.Add", builtins::decimal_add);
        inner.add_function("format", builtins::timestamp_format);

        Self {
            inner,
            clock,
            debug_vars: Vec::new(),
        }
    }
}

impl Default for CelContext {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for CelContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CelContext")
            .field("debug_vars", &self.debug_vars)
            .field("clock", &self.clock)
            .finish()
    }
}
