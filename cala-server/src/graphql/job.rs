use async_graphql::{types::connection::*, *};
use serde::{Deserialize, Serialize};

use super::primitives::*;

#[derive(SimpleObject)]
pub struct Job {
    pub id: ID,
    pub job_id: UUID,
    pub name: String,
    pub description: Option<String>,
}

#[derive(SimpleObject)]
pub struct JobCreatePayload {
    pub job: Job,
}

#[derive(Serialize, Deserialize)]
pub(super) struct JobByNameCursor {
    pub name: String,
    pub id: crate::primitives::JobId,
}

impl CursorType for JobByNameCursor {
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
