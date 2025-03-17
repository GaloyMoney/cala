use sqlx::{PgPool, Postgres, Transaction};

pub struct DbOp<'t> {
    tx: Transaction<'t, Postgres>,
    now: chrono::DateTime<chrono::Utc>,
}

impl<'t> DbOp<'t> {
    pub fn new(tx: Transaction<'t, Postgres>, time: chrono::DateTime<chrono::Utc>) -> Self {
        Self { tx, now: time }
    }

    pub async fn init(pool: &PgPool) -> Result<Self, sqlx::Error> {
        #[cfg(feature = "sim-time")]
        let res = {
            let tx = pool.begin().await?;
            let now = sim_time::now();
            Self { tx, now }
        };

        #[cfg(not(feature = "sim-time"))]
        let res = {
            let mut tx = pool.begin().await?;
            let now = sqlx::query!("SELECT NOW()")
                .fetch_one(&mut *tx)
                .await?
                .now
                .expect("NOW() is not NULL");
            Self { tx, now }
        };

        Ok(res)
    }

    pub fn now(&self) -> chrono::DateTime<chrono::Utc> {
        self.now
    }

    pub fn tx(&mut self) -> &mut Transaction<'t, Postgres> {
        &mut self.tx
    }

    pub fn into_tx(self) -> Transaction<'t, Postgres> {
        self.tx
    }

    pub async fn commit(self) -> Result<(), sqlx::Error> {
        self.tx.commit().await?;
        Ok(())
    }

    pub async fn rollback(self) -> Result<(), sqlx::Error> {
        self.tx.rollback().await?;
        Ok(())
    }
}
