use super::primitives::*;

pub(super) trait ToGlobalId {
    fn to_global_id(&self) -> async_graphql::types::ID;
}

impl From<JSON> for cala_ledger::tx_template::TxParams {
    fn from(json: JSON) -> Self {
        let mut map = Self::default();
        let inner = json.into_inner();
        if let Some(object) = inner.as_object() {
            for (k, v) in object {
                map.insert(k.clone(), v.clone());
            }
        }
        map
    }
}
