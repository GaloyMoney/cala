use sqlx::{PgPool, Postgres, Transaction};

use super::{entity::*, error::*};
use crate::entity::*;

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
        let n_new_events = events.persist(tx).await?;
        let tx_template = TxTemplate::try_from(events)?;
        Ok(EntityUpdate {
            entity: tx_template,
            n_new_events,
        })
    }
}
