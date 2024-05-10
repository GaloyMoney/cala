use sqlx::{PgPool, Postgres, Transaction};

#[cfg(feature = "import")]
use crate::primitives::DataSourceId;
use crate::{entity::*, primitives::DataSource};

use super::{entity::*, error::*};

#[derive(Debug, Clone)]
pub(super) struct TxTemplateRepo {
    _pool: PgPool,
}

impl TxTemplateRepo {
    pub fn new(pool: &PgPool) -> Self {
        Self {
            _pool: pool.clone(),
        }
    }

    pub async fn create_in_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        new_tx_template: NewTxTemplate,
    ) -> Result<EntityUpdate<TxTemplate>, TxTemplateError> {
        let id = new_tx_template.id;
        sqlx::query!(
            r#"INSERT INTO cala_tx_templates (id, code)
            VALUES ($1, $2)"#,
            id as TxTemplateId,
            new_tx_template.code,
        )
        .execute(&mut **tx)
        .await?;
        let mut events = new_tx_template.initial_events();
        let n_new_events = events.persist(tx, DataSource::Local).await?;
        let tx_template = TxTemplate::try_from(events)?;
        Ok(EntityUpdate {
            entity: tx_template,
            n_new_events,
        })
    }

    #[cfg(feature = "import")]
    pub async fn import(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        origin: DataSourceId,
        tx_template: &mut TxTemplate,
    ) -> Result<(), TxTemplateError> {
        sqlx::query!(
            r#"INSERT INTO cala_tx_templates (data_source_id, id, code)
            VALUES ($1, $2, $3)"#,
            origin as DataSourceId,
            tx_template.values().id as TxTemplateId,
            tx_template.values().code,
        )
        .execute(&mut **tx)
        .await?;
        tx_template.events.persist(tx, origin).await?;
        Ok(())
    }
}
