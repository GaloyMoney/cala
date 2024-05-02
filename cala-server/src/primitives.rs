#[derive(Debug, Clone, sqlx::Type)]
#[sqlx(transparent)]
pub struct ServerId(String);

cala_types::entity_id! { ImportJobId }
