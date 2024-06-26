use cala_es_entity::EsEntity;

#[derive(EsEntity)]
pub struct TestEntity {}

// #[derive(EntityEvent)]
enum TestEntityEvent {
    Initialized { id: u32, name: String },
}

#[test]
fn test() {
    assert_eq!(TestEntity::event_table_name(), "TestEntity");
}

// NewEntity
// EntityEvent
// EntityId
// Entity
// Repo
//
// Load
