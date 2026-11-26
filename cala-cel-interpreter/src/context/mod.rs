use std::borrow::Cow;

use cel::Context;
use es_entity::clock::{Clock, ClockHandle};

use crate::{builtins, value::CelValue};

pub struct CelContext {
    inner: Context<'static, 'static>,
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

    pub(crate) fn inner(&self) -> &Context<'static, 'static> {
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
        inner
            .add_function("date", move |args| builtins::date(date_clock.clone(), args))
            .expect("failed to register CEL function date");
        inner
            .add_function("uuid", builtins::uuid)
            .expect("failed to register CEL function uuid");
        inner
            .add_function("decimal", builtins::decimal)
            .expect("failed to register CEL function decimal");
        inner
            .add_function("decimal.Add", builtins::decimal_add)
            .expect("failed to register CEL function decimal.Add");
        inner
            .add_function("decimal.Sub", builtins::decimal_sub)
            .expect("failed to register CEL function decimal.Sub");
        inner
            .add_function("decimal.Mul", builtins::decimal_mul)
            .expect("failed to register CEL function decimal.Mul");
        inner
            .add_function("decimal.Cmp", builtins::decimal_cmp)
            .expect("failed to register CEL function decimal.Cmp");
        inner
            .add_function("format", builtins::timestamp_format)
            .expect("failed to register CEL function format");

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
