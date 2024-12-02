use chrono::{DateTime, Utc};

use super::{error::EsEntityError, traits::*};

pub type LastPersisted<'a, E> = std::slice::Iter<'a, PersistedEvent<E>>;

pub struct GenericEvent<Id> {
    pub entity_id: Id,
    pub sequence: i32,
    pub event: serde_json::Value,
    pub recorded_at: DateTime<Utc>,
}

pub struct PersistedEvent<E: EsEvent> {
    pub entity_id: <E as EsEvent>::EntityId,
    pub recorded_at: DateTime<Utc>,
    pub sequence: usize,
    pub event: E,
}

impl<E: Clone + EsEvent> Clone for PersistedEvent<E> {
    fn clone(&self) -> Self {
        PersistedEvent {
            entity_id: self.entity_id.clone(),
            recorded_at: self.recorded_at,
            sequence: self.sequence,
            event: self.event.clone(),
        }
    }
}

pub struct EntityEvents<T: EsEvent> {
    pub entity_id: <T as EsEvent>::EntityId,
    persisted_events: Vec<PersistedEvent<T>>,
    new_events: Vec<T>,
}

impl<T: Clone + EsEvent> Clone for EntityEvents<T> {
    fn clone(&self) -> Self {
        Self {
            entity_id: self.entity_id.clone(),
            persisted_events: self.persisted_events.clone(),
            new_events: self.new_events.clone(),
        }
    }
}

impl<T> EntityEvents<T>
where
    T: EsEvent,
{
    pub fn init(id: <T as EsEvent>::EntityId, initial_events: impl IntoIterator<Item = T>) -> Self {
        Self {
            entity_id: id,
            persisted_events: Vec::new(),
            new_events: initial_events.into_iter().collect(),
        }
    }

    pub fn id(&self) -> &<T as EsEvent>::EntityId {
        &self.entity_id
    }

    pub fn entity_first_persisted_at(&self) -> Option<DateTime<Utc>> {
        self.persisted_events.first().map(|e| e.recorded_at)
    }

    pub fn entity_last_modified_at(&self) -> Option<DateTime<Utc>> {
        self.persisted_events.last().map(|e| e.recorded_at)
    }

    pub fn push(&mut self, event: T) {
        self.new_events.push(event);
    }

    pub fn mark_new_events_persisted_at(
        &mut self,
        recorded_at: chrono::DateTime<chrono::Utc>,
    ) -> usize {
        let n = self.new_events.len();
        let offset = self.persisted_events.len() + 1;
        self.persisted_events
            .extend(
                self.new_events
                    .drain(..)
                    .enumerate()
                    .map(|(i, event)| PersistedEvent {
                        entity_id: self.entity_id.clone(),
                        recorded_at,
                        sequence: i + offset,
                        event,
                    }),
            );
        n
    }

    pub fn serialize_new_events(&self) -> Vec<serde_json::Value> {
        self.new_events
            .iter()
            .map(|event| serde_json::to_value(event).expect("Failed to serialize event"))
            .collect()
    }

    pub fn any_new(&self) -> bool {
        !self.new_events.is_empty()
    }

    pub fn len_persisted(&self) -> usize {
        self.persisted_events.len()
    }

    pub fn iter_persisted(&self) -> impl DoubleEndedIterator<Item = &PersistedEvent<T>> {
        self.persisted_events.iter()
    }

    pub fn last_persisted(&self, n: usize) -> LastPersisted<T> {
        let start = self.persisted_events.len() - n;
        self.persisted_events[start..].iter()
    }

    pub fn iter_all(&self) -> impl DoubleEndedIterator<Item = &T> {
        self.persisted_events
            .iter()
            .map(|e| &e.event)
            .chain(self.new_events.iter())
    }

    pub fn load_first<E: EsEntity<Event = T>>(
        events: impl IntoIterator<Item = GenericEvent<<T as EsEvent>::EntityId>>,
    ) -> Result<E, EsEntityError> {
        let mut current_id = None;
        let mut current = None;
        for e in events {
            if current_id.is_none() {
                current_id = Some(e.entity_id.clone());
                current = Some(Self {
                    entity_id: e.entity_id.clone(),
                    persisted_events: Vec::new(),
                    new_events: Vec::new(),
                });
            }
            if current_id.as_ref() != Some(&e.entity_id) {
                break;
            }
            let cur = current.as_mut().expect("Could not get current");
            cur.persisted_events.push(PersistedEvent {
                entity_id: e.entity_id,
                recorded_at: e.recorded_at,
                sequence: e.sequence as usize,
                event: serde_json::from_value(e.event)?,
            });
        }
        if let Some(current) = current {
            E::try_from_events(current)
        } else {
            Err(EsEntityError::NotFound)
        }
    }

    pub fn load_n<E: EsEntity<Event = T>>(
        events: impl IntoIterator<Item = GenericEvent<<T as EsEvent>::EntityId>>,
        n: usize,
    ) -> Result<(Vec<E>, bool), EsEntityError> {
        let mut ret: Vec<E> = Vec::new();
        let mut current_id = None;
        let mut current = None;
        for e in events {
            if current_id.as_ref() != Some(&e.entity_id) {
                if let Some(current) = current.take() {
                    ret.push(E::try_from_events(current)?);
                    if ret.len() == n {
                        return Ok((ret, true));
                    }
                }

                current_id = Some(e.entity_id.clone());
                current = Some(Self {
                    entity_id: e.entity_id.clone(),
                    persisted_events: Vec::new(),
                    new_events: Vec::new(),
                });
            }
            let cur = current.as_mut().expect("Could not get current");
            cur.persisted_events.push(PersistedEvent {
                entity_id: e.entity_id,
                recorded_at: e.recorded_at,
                sequence: e.sequence as usize,
                event: serde_json::from_value(e.event)?,
            });
        }
        if let Some(current) = current.take() {
            ret.push(E::try_from_events(current)?);
        }
        Ok((ret, false))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[derive(Debug, serde::Serialize, serde::Deserialize)]
    enum DummyEntityEvent {
        Created(String),
    }

    impl EsEvent for DummyEntityEvent {
        type EntityId = Uuid;
    }

    struct DummyEntity {
        name: String,

        events: EntityEvents<DummyEntityEvent>,
    }

    impl EsEntity for DummyEntity {
        type Event = DummyEntityEvent;
        type New = NewDummyEntity;

        fn events_mut(&mut self) -> &mut EntityEvents<DummyEntityEvent> {
            &mut self.events
        }
        fn events(&self) -> &EntityEvents<DummyEntityEvent> {
            &self.events
        }
    }

    impl TryFromEvents<DummyEntityEvent> for DummyEntity {
        fn try_from_events(events: EntityEvents<DummyEntityEvent>) -> Result<Self, EsEntityError> {
            let name = events
                .iter_persisted()
                .map(|e| match &e.event {
                    DummyEntityEvent::Created(name) => name.clone(),
                })
                .next()
                .expect("Could not find name");
            Ok(Self { name, events })
        }
    }

    struct NewDummyEntity {}

    impl IntoEvents<DummyEntityEvent> for NewDummyEntity {
        fn into_events(self) -> EntityEvents<DummyEntityEvent> {
            EntityEvents::init(
                Uuid::new_v4(),
                vec![DummyEntityEvent::Created("".to_owned())],
            )
        }
    }

    #[test]
    fn load_zero_events() {
        let generic_events = vec![];
        let res = EntityEvents::load_first::<DummyEntity>(generic_events);
        assert!(matches!(res, Err(EsEntityError::NotFound)));
    }

    #[test]
    fn load_first() {
        let generic_events = vec![GenericEvent {
            entity_id: uuid::Uuid::new_v4(),
            sequence: 1,
            event: serde_json::to_value(DummyEntityEvent::Created("dummy-name".to_owned()))
                .expect("Could not serialize"),
            recorded_at: chrono::Utc::now(),
        }];
        let entity: DummyEntity = EntityEvents::load_first(generic_events).expect("Could not load");
        assert!(entity.name == "dummy-name");
    }

    #[test]
    fn load_n() {
        let generic_events = vec![
            GenericEvent {
                entity_id: uuid::Uuid::new_v4(),
                sequence: 1,
                event: serde_json::to_value(DummyEntityEvent::Created("dummy-name".to_owned()))
                    .expect("Could not serialize"),
                recorded_at: chrono::Utc::now(),
            },
            GenericEvent {
                entity_id: uuid::Uuid::new_v4(),
                sequence: 1,
                event: serde_json::to_value(DummyEntityEvent::Created("other-name".to_owned()))
                    .expect("Could not serialize"),
                recorded_at: chrono::Utc::now(),
            },
        ];
        let (entity, more): (Vec<DummyEntity>, _) =
            EntityEvents::load_n(generic_events, 2).expect("Could not load");
        assert!(!more);
        assert_eq!(entity.len(), 2);
    }
}
