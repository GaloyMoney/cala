pub use cel_interpreter::CelContext;
use es_entity::clock::ClockHandle;
use tracing::instrument;

#[instrument(name = "cel_context.initialize", skip(clock))]
pub(crate) fn initialize(clock: ClockHandle) -> CelContext {
    let mut ctx = CelContext::new_with_clock(clock);
    ctx.add_variable("SETTLED", "SETTLED");
    ctx.add_variable("PENDING", "PENDING");
    ctx.add_variable("ENCUMBRANCE", "ENCUMBRANCE");
    ctx.add_variable("DEBIT", "DEBIT");
    ctx.add_variable("CREDIT", "CREDIT");
    ctx
}
