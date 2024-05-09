use uuid::Uuid;

pub struct CurrentJob {
    pub id: Uuid,
}

impl CurrentJob {
    pub(super) fn new(id: Uuid) -> Self {
        Self { id }
    }
}
