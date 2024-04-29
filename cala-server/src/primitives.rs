#[derive(Debug, Clone, sqlx::Type)]
#[sqlx(transparent)]
pub struct ServerId(String);
