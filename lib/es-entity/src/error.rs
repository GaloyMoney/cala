use thiserror::Error;

#[derive(Error, Debug)]
pub enum EsEntityError {
    #[error("EsEntityError - UninitializedFieldError: {0}")]
    UninitializedFieldError(#[from] derive_builder::UninitializedFieldError),
    #[error("EsEntityError - Deserialization: {0}")]
    EventDeserialization(#[from] serde_json::Error),
    #[error("EntityError - NotFound")]
    NotFound,
    #[error("EntityError - ConcurrentModification")]
    ConcurrentModification,
}

#[derive(Error, Debug)]
#[error("CursorDestructureError: couldn't turn {0} into {1}")]
pub struct CursorDestructureError(&'static str, &'static str);

impl From<(&'static str, &'static str)> for CursorDestructureError {
    fn from((name, variant): (&'static str, &'static str)) -> Self {
        Self(name, variant)
    }
}

#[derive(Error, Debug)]
pub enum EsRepoError {
    #[error("EsRepoError - Sqlx: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("{0}")]
    EsEntityError(EsEntityError),
    #[error("{0}")]
    CursorDestructureError(#[from] CursorDestructureError),
}

crate::from_es_entity_error!(EsRepoError);
