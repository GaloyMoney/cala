pub use cel_interpreter::CelContext;
use tracing::instrument;

#[instrument(name = "cel_context.initialize")]
pub(crate) fn initialize() -> CelContext {
    let mut ctx = CelContext::new();
    ctx.add_variable("SETTLED", "SETTLED");
    ctx.add_variable("PENDING", "PENDING");
    ctx.add_variable("ENCUMBRANCE", "ENCUMBRANCE");
    ctx.add_variable("DEBIT", "DEBIT");
    ctx.add_variable("CREDIT", "CREDIT");
    ctx
}
