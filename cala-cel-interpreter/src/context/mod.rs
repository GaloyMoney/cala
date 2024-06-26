mod decimal;

use std::collections::HashMap;

use crate::{builtins, error::*, value::*};

const SELF_PACKAGE_NAME: &str = "self";

type CelFunction = Box<dyn Fn(Vec<CelValue>) -> Result<CelValue, CelError> + Sync>;
#[derive(Debug)]
pub struct CelContext {
    idents: HashMap<String, ContextItem>,
}

impl CelContext {
    pub fn add_variable(&mut self, name: impl Into<String>, value: impl Into<CelValue>) {
        self.idents
            .insert(name.into(), ContextItem::Value(value.into()));
    }

    pub fn new() -> Self {
        let mut idents = HashMap::new();
        idents.insert(
            "date".to_string(),
            ContextItem::Function(Box::new(builtins::date)),
        );
        idents.insert(
            "uuid".to_string(),
            ContextItem::Function(Box::new(builtins::uuid)),
        );
        idents.insert(
            "decimal".to_string(),
            ContextItem::Package(&decimal::CEL_CONTEXT),
        );
        Self { idents }
    }

    pub(crate) fn package_self(&self) -> Result<&ContextItem, CelError> {
        self.lookup(SELF_PACKAGE_NAME)
    }

    pub(crate) fn lookup(&self, name: &str) -> Result<&ContextItem, CelError> {
        self.idents
            .get(name)
            .ok_or_else(|| CelError::UnknownIdent(name.to_string()))
    }
}
impl Default for CelContext {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) enum ContextItem {
    Value(CelValue),
    Function(CelFunction),
    Package(&'static CelContext),
}

impl std::fmt::Debug for ContextItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContextItem::Value(val) => write!(f, "Value({val:?})"),
            ContextItem::Function(_) => write!(f, "Function"),
            ContextItem::Package(_) => write!(f, "Package"),
        }
    }
}
