use std::{borrow::Cow, collections::HashMap, sync::LazyLock};

use crate::builtins;

use super::*;

pub static CEL_PACKAGE: LazyLock<CelPackage> = LazyLock::new(|| {
    let mut idents = HashMap::new();
    idents.insert(
        SELF_PACKAGE_NAME,
        ContextItem::Function(Box::new(|_ctx, args| builtins::decimal::cast(args))),
    );
    idents.insert(
        Cow::Borrowed("Add"),
        ContextItem::Function(Box::new(|_ctx, args| builtins::decimal::add(args))),
    );

    CelPackage::new(
        CelContext {
            idents,
            clock: Clock::handle().clone(),
        },
        HashMap::new(),
    )
});
