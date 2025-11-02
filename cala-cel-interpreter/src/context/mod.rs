mod decimal;
mod package;
mod timestamp;

use std::{borrow::Cow, collections::HashMap};

use crate::{builtins, cel_type::CelType, error::*, value::*};

use package::CelPackage;

const SELF_PACKAGE_NAME: Cow<'static, str> = Cow::Borrowed("self");

type CelFunction = Box<dyn Fn(Vec<CelValue>) -> Result<CelValue, CelError> + Sync>;
pub(crate) type CelMemberFunction =
    Box<dyn Fn(&CelValue, Vec<CelValue>) -> Result<CelValue, CelError> + Sync>;

#[derive(Debug)]
pub struct CelContext {
    idents: HashMap<Cow<'static, str>, ContextItem>,
}

impl CelContext {
    pub fn add_variable(&mut self, name: impl Into<Cow<'static, str>>, value: impl Into<CelValue>) {
        self.idents
            .insert(name.into(), ContextItem::Value(value.into()));
    }

    /// Returns a debug representation of all context variables with their values
    /// Useful for tracing/debugging - excludes built-in functions and packages
    pub fn debug_context(&self) -> String {
        let vars: Vec<_> = self
            .idents
            .iter()
            .filter_map(|(name, item)| match item {
                ContextItem::Value(val) => Some(format!("{}={:?}", name, val)),
                _ => None,
            })
            .collect();

        if vars.is_empty() {
            String::new()
        } else {
            vars.join(", ")
        }
    }

    pub fn new() -> Self {
        let mut idents = HashMap::new();
        idents.insert(
            Cow::Borrowed("date"),
            ContextItem::Function(Box::new(builtins::date)),
        );
        idents.insert(
            Cow::Borrowed("uuid"),
            ContextItem::Function(Box::new(builtins::uuid)),
        );
        idents.insert(
            Cow::Borrowed("decimal"),
            ContextItem::Package(&decimal::CEL_PACKAGE),
        );

        idents.insert(
            Cow::Borrowed("timestamp"),
            ContextItem::Package(&timestamp::CEL_PACKAGE),
        );

        Self { idents }
    }

    pub(crate) fn lookup_ident(&self, name: &str) -> Result<&ContextItem, CelError> {
        self.idents
            .get(name)
            .ok_or_else(|| CelError::UnknownIdent(name.to_string()))
    }

    pub(crate) fn lookup_member_fn(
        &self,
        value: &CelValue,
        name: &str,
    ) -> Result<&CelMemberFunction, CelError> {
        let package_name = CelType::from(value).package_name();
        let package = if let Some(ContextItem::Package(package)) = self.idents.get(package_name) {
            package
        } else {
            return Err(CelError::UnknownPackage(package_name));
        };

        package.lookup_member(value, name)
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
    Package(&'static CelPackage),
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
