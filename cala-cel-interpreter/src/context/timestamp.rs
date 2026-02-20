use std::{collections::HashMap, sync::LazyLock};

use crate::builtins;

use super::*;

pub static CEL_PACKAGE: LazyLock<CelPackage> = LazyLock::new(|| {
    let mut idents = HashMap::new();
    idents.insert(
        SELF_PACKAGE_NAME,
        ContextItem::Function(Box::new(|_ctx, args| builtins::timestamp::cast(args))),
    );

    let mut member_fns: HashMap<&'static str, CelMemberFunction> = HashMap::new();
    member_fns.insert("format", Box::new(builtins::timestamp::format));

    CelPackage::new(
        CelContext {
            idents,
            clock: Clock::handle().clone(),
        },
        member_fns,
    )
});
