use lazy_static::lazy_static;

use std::collections::HashMap;

use crate::builtins;

use super::*;

lazy_static! {
    pub static ref CEL_CONTEXT: CelContext = {
        let mut idents = HashMap::new();
        idents.insert(
            SELF_PACKAGE_NAME.to_string(),
            ContextItem::Function(Box::new(builtins::decimal::cast)),
        );
        idents.insert(
            "Add".to_string(),
            ContextItem::Function(Box::new(builtins::decimal::add)),
        );
        CelContext { idents }
    };
}
