use lazy_static::lazy_static;

use std::collections::HashMap;

use crate::builtins;

use super::*;

lazy_static! {
    pub static ref CEL_PACKAGE: CelPackage = {
        let mut idents = HashMap::new();
        idents.insert(
            SELF_PACKAGE_NAME,
            ContextItem::Function(Box::new(builtins::timestamp::cast)),
        );

        let mut member_fns: HashMap<_, CelMemberFunction> = HashMap::new();
        member_fns.insert("format", Box::new(builtins::timestamp::format));

        CelPackage::new(CelContext { idents }, member_fns)
    };
}
