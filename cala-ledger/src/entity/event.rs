use serde::{de::DeserializeOwned, Serialize};

pub trait EntityEvent {
    type EntityId: Into<uuid::Uuid> + Copy;

    fn event_table_name() -> &'static str
    where
        Self: Sized;
}

pub(crate) struct EntityUpdate<T: EntityEvent> {
    pub id: <T as EntityEvent>::EntityId,
    pub new_events: Vec<T>,
}

pub struct EntityEvents<T: DeserializeOwned + Serialize + EntityEvent> {
    entity_id: <T as EntityEvent>::EntityId,
    new_events: Vec<T>,
}

impl<T> EntityEvents<T>
where
    T: DeserializeOwned + Serialize + 'static + EntityEvent,
{
    pub fn init(
        id: <T as EntityEvent>::EntityId,
        initial_events: impl IntoIterator<Item = T>,
    ) -> Self {
        Self {
            entity_id: id,
            new_events: initial_events.into_iter().collect(),
        }
    }

    pub async fn persist(
        &mut self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<EntityUpdate<T>, sqlx::Error> {
        let uuid: uuid::Uuid = self.entity_id.into();
        let mut events = Vec::new();
        std::mem::swap(&mut events, &mut self.new_events);
        let mut query_builder = sqlx::QueryBuilder::new(format!(
            "INSERT INTO {} (id, sequence, event_type, event)",
            <T as EntityEvent>::event_table_name(),
        ));
        let sequence = 1;
        query_builder.push_values(events.iter().enumerate(), |mut builder, (offset, event)| {
            let event_json = serde_json::to_value(event).expect("Could not serialize event");
            let event_type = event_json
                .get("type")
                .and_then(serde_json::Value::as_str)
                .expect("Could not get type")
                .to_owned();
            builder.push_bind(uuid);
            builder.push_bind(sequence + offset as i32);
            builder.push_bind(event_type);
            builder.push_bind(event_json);
        });

        let query = query_builder.build();
        query.execute(&mut **tx).await?;

        Ok(EntityUpdate {
            id: self.entity_id,
            new_events: events,
        })
    }
}
