use thiserror::Error;

#[derive(Error, Debug)]
pub enum JournalError {
    #[error("JournalError - Sqlx: {0}")]
    Sqlx(sqlx::Error),
    #[error("JournalError - EsEntityError: {0}")]
    EsEntityError(es_entity::EsEntityError),
    #[error("JournalError - CursorDestructureError: {0}")]
    CursorDestructureError(#[from] es_entity::CursorDestructureError),
    #[error("JournalError - code already exists")]
    CodeAlreadyExists,
}

impl From<sqlx::Error> for JournalError {
    fn from(error: sqlx::Error) -> Self {
        if let Some(err) = error.as_database_error() {
            if let Some(constraint) = err.constraint() {
                if constraint.contains("code") {
                    return Self::CodeAlreadyExists;
                }
            }
        }
        Self::Sqlx(error)
    }
}

es_entity::from_es_entity_error!(JournalError);
