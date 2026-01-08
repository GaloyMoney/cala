use lazy_static::lazy_static;

use std::{borrow::Cow, collections::HashMap};

use crate::builtins;

use super::*;

lazy_static! {
    pub static ref CEL_PACKAGE: CelPackage = {
        let mut idents = HashMap::new();
        idents.insert(
            SELF_PACKAGE_NAME,
            ContextItem::Function(Box::new(|_ctx, args| builtins::decimal::cast(args))),
        );
        idents.insert(
            Cow::Borrowed("Add"),
            ContextItem::Function(Box::new(|_ctx, args| builtins::decimal::add(args))),
        );

        CelPackage::new(CelContext { idents }, HashMap::new())
    };
}
