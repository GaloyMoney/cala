use sqlx::{PgPool, Postgres, Transaction};

use std::collections::HashMap;

use cala_types::account::AccountValues;

use crate::primitives::{AccountId, VelocityControlId};

use super::{super::error::*, value::*};

#[derive(Debug, Clone)]
pub struct AccountControlRepo {
    _pool: PgPool,
}

impl AccountControlRepo {
    pub fn new(pool: &PgPool) -> Self {
        Self {
            _pool: pool.clone(),
        }
    }

    pub async fn create_in_tx(
        &self,
        db: &mut Transaction<'_, Postgres>,
        control: AccountVelocityControl,
    ) -> Result<(), VelocityError> {
        sqlx::query!(
            r#"INSERT INTO cala_velocity_account_controls (account_id, velocity_control_id, values)
            VALUES ($1, $2, $3)"#,
            control.account_id as AccountId,
            control.control_id as VelocityControlId,
            serde_json::to_value(control).expect("Failed to serialize control values"),
        )
        .execute(&mut **db)
        .await?;
        Ok(())
    }

    pub async fn find_for_enforcement(
        &self,
        db: &mut Transaction<'_, Postgres>,
        account_ids: &[AccountId],
    ) -> Result<HashMap<AccountId, (AccountValues, Vec<AccountVelocityControl>)>, VelocityError>
    {
        let rows = sqlx::query!(
            r#"SELECT values, latest_values
            FROM cala_velocity_account_controls v
            JOIN cala_accounts a
            ON v.account_id = a.id
              AND v.data_source_id = a.data_source_id
            WHERE v.data_source_id = '00000000-0000-0000-0000-000000000000'
              AND account_id = ANY($1)"#,
            account_ids as &[AccountId],
        )
        .fetch_all(&mut **db)
        .await?;

        let mut res: HashMap<AccountId, (AccountValues, Vec<_>)> = HashMap::new();

        for row in rows {
            let values: AccountVelocityControl =
                serde_json::from_value(row.values).expect("Failed to deserialize control values");
            res.entry(values.account_id)
                .or_insert_with(|| {
                    (
                        serde_json::from_value(row.latest_values)
                            .expect("Failed to deserialize account values"),
                        Vec::new(),
                    )
                })
                .1
                .push(values);
        }

        Ok(res)
    }
}
