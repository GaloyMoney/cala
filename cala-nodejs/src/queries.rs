#[napi(object)]
#[derive(Clone)]
pub struct CursorToken {
    pub token: String,
}

#[napi(object)]
pub struct PaginatedQueryArgs {
    pub after: Option<CursorToken>,
    pub first: i32,
}

impl From<cala_types::query::AccountByNameCursor> for CursorToken {
    fn from(cursor: cala_types::query::AccountByNameCursor) -> Self {
        use base64::{engine::general_purpose, Engine as _};
        let json = serde_json::to_string(&cursor).expect("could not serialize token");
        let token: String = general_purpose::STANDARD_NO_PAD.encode(json.as_bytes());
        CursorToken { token }
    }
}
impl TryFrom<CursorToken> for cala_types::query::AccountByNameCursor {
    type Error = napi::Error;

    fn try_from(token: CursorToken) -> Result<Self, Self::Error> {
        use base64::{engine::general_purpose, Engine as _};
        let json_bytes = general_purpose::STANDARD_NO_PAD
            .decode(token.token)
            .map_err(crate::generic_napi_error)?;
        let json = String::from_utf8(json_bytes).map_err(crate::generic_napi_error)?;
        serde_json::from_str(&json).map_err(crate::generic_napi_error)
    }
}
