use cala_cel_interpreter::CelContext;

pub(super) fn initialize() -> CelContext {
    let mut ctx = CelContext::new();
    ctx.add_variable("SETTLED", "SETTLED");
    ctx.add_variable("PENDING", "PENDING");
    ctx.add_variable("ENCUMBERED", "ENCUMBERED");
    ctx.add_variable("DEBIT", "DEBIT");
    ctx.add_variable("CREDIT", "CREDIT");
    ctx
}
