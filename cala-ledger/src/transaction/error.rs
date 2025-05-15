use regex::Regex;
use thiserror::Error;

use cala_types::primitives::TransactionId;

#[derive(Error, Debug)]
pub enum TransactionError {
    #[error("TransactionError - Sqlx: {0}")]
    Sqlx(sqlx::Error),
    #[error("TransactionError - NotFound: external id '{0}' not found")]
    CouldNotFindByExternalId(String),
    #[error("TransactionError - NotFound: id '{0}' not found")]
    CouldNotFindById(TransactionId),
    #[error("TransactionError - EsEntityError: {0}")]
    EsEntityError(es_entity::EsEntityError),
    #[error("TransactionError - CursorDestructureError: {0}")]
    CursorDestructureError(#[from] es_entity::CursorDestructureError),
    #[error("TransactionError - DuplicateExternalId: external_id '{0}' already exists")]
    DuplicateExternalId(String),
    #[error("TransactionError - DuplicateId: id '{0}' already exists")]
    DuplicateId(String),
}

impl From<sqlx::Error> for TransactionError {
    fn from(e: sqlx::Error) -> Self {
        match e {
            sqlx::Error::Database(ref err) if err.is_unique_violation() => {
                if err.constraint().is_none() {
                    return Self::Sqlx(e);
                };

                let detail = err
                    .downcast_ref::<sqlx::postgres::PgDatabaseError>()
                    .detail();

                if let Some(detail_msg) = detail {
                    let re = Regex::new(r"Key \((?P<field>[^)]+)\)=\((?P<value>[^)]+)\)").unwrap();

                    if let Some(caps) = re.captures(detail_msg) {
                        let field = &caps["field"];
                        let value = &caps["value"];

                        match field {
                            "external_id" => return Self::DuplicateExternalId(value.to_string()),
                            "id" => return Self::DuplicateId(value.to_string()),
                            _ => {}
                        }
                    }
                }

                Self::Sqlx(e)
            }
            e => Self::Sqlx(e),
        }
    }
}

es_entity::from_es_entity_error!(TransactionError);
