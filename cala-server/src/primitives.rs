#[derive(Debug, Clone, sqlx::Type)]
#[sqlx(transparent)]
pub struct ServerId(String);

es_entity::entity_id! { ImportJobId }
es_entity::entity_id! { JobId }
