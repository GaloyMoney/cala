mod error;

use sqlx::{Pool, Postgres};

pub use error::*;

#[derive(Clone)]
pub struct CalaApp {
    pool: Pool<Postgres>,
}

impl CalaApp {
    pub fn new(pool: Pool<Postgres>) -> Self {
        Self { pool }
    }
}
