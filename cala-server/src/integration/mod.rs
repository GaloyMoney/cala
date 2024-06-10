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
    data: serde_json::Value,
}

impl Integration {
    fn new(id: IntegrationId, name: String, data: impl serde::Serialize) -> Self {
        Self {
            id,
            name,
            data: serde_json::to_value(data).expect("Could not serialize data"),
        }
    }
    pub fn data<T: serde::de::DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        serde_json::from_value(self.data.clone())
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
    ) -> Result<Integration, sqlx::Error> {
        let integration = Integration::new(id.into(), name, data);
        let (cipher, nonce) = integration.encrypt(&self.encryption_config.key)?;
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

        let data = Integration::decrypt(
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
