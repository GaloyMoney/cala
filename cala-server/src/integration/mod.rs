use serde::{Deserialize, Serialize};

mod encryption_config;
pub mod error;

pub use encryption_config::*;
use error::IntegrationError;

use sqlx::PgPool;

use cala_ledger::AtomicOperation;

cala_types::entity_id! { IntegrationId }

pub struct Integration {
    pub id: IntegrationId,
    pub name: String,
    data: Data,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct Data(serde_json::Value);

impl Data {
    fn new(data: impl serde::Serialize) -> Self {
        Self(serde_json::to_value(data).unwrap())
    }
}

impl AsRef<serde_json::Value> for Data {
    fn as_ref(&self) -> &serde_json::Value {
        &self.0
    }
}

impl Integration {
    fn new(id: IntegrationId, name: String, data: impl serde::Serialize) -> Self {
        Self {
            id,
            name,
            data: Data::new(data),
        }
    }
    pub fn data<T: serde::de::DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        serde_json::from_value(self.data.as_ref().clone())
    }
}

pub struct Integrations {
    pool: PgPool,
    encryption_config: EncryptionConfig,
}

impl Integrations {
    pub(crate) fn new(pool: &PgPool, encryption_config: &EncryptionConfig) -> Self {
        Self {
            pool: pool.clone(),
            encryption_config: encryption_config.clone(),
        }
    }

    pub async fn create_in_op(
        &self,
        op: &mut AtomicOperation<'_>,
        id: impl Into<IntegrationId> + std::fmt::Debug,
        name: String,
        data: impl serde::Serialize,
    ) -> Result<Integration, IntegrationError> {
        let integration = Integration::new(id.into(), name, data);
        let (cipher, nonce) = integration.data.encrypt(&self.encryption_config.key)?;
        sqlx::query!(
            r#"INSERT INTO integrations (id, name, cipher, nonce)
            VALUES ($1, $2, $3, $4)"#,
            integration.id as IntegrationId,
            integration.name,
            &cipher.0,
            &nonce.0
        )
        .execute(&mut **op.tx())
        .await?;
        Ok(integration)
    }

    pub async fn find_by_id(
        &self,
        id: impl Into<IntegrationId>,
    ) -> Result<Integration, IntegrationError> {
        let id = id.into();
        let row = sqlx::query!(
            r#"SELECT id, name, cipher, nonce 
            FROM integrations
            WHERE id = $1"#,
            id as IntegrationId
        )
        .fetch_one(&self.pool)
        .await?;

        let data = Data::decrypt(
            &ConfigCipher(row.cipher),
            &Nonce(row.nonce),
            &self.encryption_config.key,
        )?;
        Ok(Integration {
            id,
            name: row.name,
            data,
        })
    }
}
