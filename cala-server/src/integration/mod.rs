use sqlx::PgPool;

use cala_ledger::AtomicOperation;

cala_types::entity_id! { IntegrationId }

pub struct Integration {
    pub id: IntegrationId,
    pub name: String,
    config: serde_json::Value,
}

impl Integration {
    fn new(name: String, config: impl serde::Serialize) -> Self {
        Self {
            id: IntegrationId::new(),
            name,
            config: serde_json::to_value(config).expect("Could not serialize config"),
        }
    }
    pub fn config<T: serde::de::DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        serde_json::from_value(self.config.clone())
    }
}

pub struct Integrations {
    pool: PgPool,
}

impl Integrations {
    pub(crate) fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn create_in_op(
        &self,
        op: &mut AtomicOperation<'_>,
        name: String,
        config: impl serde::Serialize,
    ) -> Result<Integration, sqlx::Error> {
        let integration = Integration::new(name, config);
        sqlx::query!(
            r#"INSERT INTO integrations (id, name, config)
            VALUES ($1, $2, $3)"#,
            integration.id as IntegrationId,
            integration.name,
            integration.config
        )
        .execute(&mut **op.tx())
        .await?;
        Ok(integration)
    }

    pub async fn find_by_id(
        &self,
        id: impl Into<IntegrationId>,
    ) -> Result<Integration, sqlx::Error> {
        let id = id.into();
        let row = sqlx::query_as!(
            Integration,
            r#"SELECT id, name, config
            FROM integrations
            WHERE id = $1"#,
            id as IntegrationId
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }
}
