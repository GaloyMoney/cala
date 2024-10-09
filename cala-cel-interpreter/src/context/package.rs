use super::*;

pub struct CelPackage {
    nested_ctx: CelContext,
    member_fns: HashMap<&'static str, CelMemberFunction>,
}

impl CelPackage {
    pub fn new(
        nested_ctx: CelContext,
        member_fns: HashMap<&'static str, CelMemberFunction>,
    ) -> Self {
        Self {
            nested_ctx,
            member_fns,
        }
    }

    pub(crate) fn package_self(&self) -> Result<&ContextItem, CelError> {
        self.nested_ctx.lookup_ident(&SELF_PACKAGE_NAME)
    }

    pub(crate) fn lookup(&self, name: &str) -> Result<&ContextItem, CelError> {
        self.nested_ctx.lookup_ident(name)
    }

    pub(crate) fn lookup_member(
        &self,
        value: &CelValue,
        name: &str,
    ) -> Result<&CelMemberFunction, CelError> {
        self.member_fns
            .get(name)
            .ok_or_else(|| CelError::UnknownAttribute(CelType::from(value), name.to_string()))
    }
}
