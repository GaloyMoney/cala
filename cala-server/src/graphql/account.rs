use async_graphql::{types::connection::*, *};
use serde::{Deserialize, Serialize};

use super::primitives::*;

#[derive(SimpleObject)]
pub(super) struct Account {
    pub id: ID,
    pub account_id: UUID,
    pub code: String,
    pub name: String,
    pub normal_balance_type: DebitOrCredit,
    pub status: Status,
    pub external_id: Option<String>,
    pub description: Option<String>,
    pub tags: Vec<TAG>,
    pub metadata: Option<JSON>,
}

#[derive(Serialize, Deserialize)]
pub(super) struct AccountByNameCursor {
    pub name: String,
    pub id: cala_ledger::primitives::AccountId,
}

impl CursorType for AccountByNameCursor {
    type Error = String;

    fn encode_cursor(&self) -> String {
        use base64::{engine::general_purpose, Engine as _};
        let json = serde_json::to_string(&self).expect("could not serialize token");
        general_purpose::STANDARD_NO_PAD.encode(json.as_bytes())
    }

    fn decode_cursor(s: &str) -> Result<Self, Self::Error> {
        use base64::{engine::general_purpose, Engine as _};
        let bytes = general_purpose::STANDARD_NO_PAD
            .decode(s.as_bytes())
            .map_err(|e| e.to_string())?;
        let json = String::from_utf8(bytes).map_err(|e| e.to_string())?;
        serde_json::from_str(&json).map_err(|e| e.to_string())
    }
}

#[derive(InputObject)]
pub(super) struct AccountCreateInput {
    pub id: Option<UUID>,
    pub external_id: Option<String>,
    pub code: String,
    pub name: String,
    #[graphql(default)]
    pub normal_balance_type: DebitOrCredit,
    pub description: Option<String>,
    #[graphql(default)]
    pub status: Status,
    #[graphql(default)]
    pub tags: Vec<TAG>,
    pub metadata: Option<JSON>,
}

#[derive(SimpleObject)]
pub(super) struct AccountCreatePayload {
    pub account: Account,
}
