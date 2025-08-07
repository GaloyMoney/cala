use sqlx::PgPool;

use cala_ledger::LedgerOperation;

es_entity::entity_id! { IntegrationId }

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
}

impl Integrations {
    pub(crate) fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn create_in_op(
        &self,
        op: &mut LedgerOperation<'_>,
        id: impl Into<IntegrationId> + std::fmt::Debug,
        name: String,
        data: impl serde::Serialize,
    ) -> Result<Integration, sqlx::Error> {
        use es_entity::AtomicOperation;

        let integration = Integration::new(id.into(), name, data);
        sqlx::query!(
            r#"INSERT INTO integrations (id, name, data)
            VALUES ($1, $2, $3)"#,
            integration.id as IntegrationId,
            integration.name,
            integration.data
        )
        .execute(op.as_executor())
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
            r#"SELECT id, name, data
            FROM integrations
            WHERE id = $1"#,
            id as IntegrationId
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }
}
