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
        CursorToken {
            token: serde_json::to_string(&cursor).expect("could not serialize token"),
        }
    }
}
impl TryFrom<CursorToken> for cala_types::query::AccountByNameCursor {
    type Error = napi::Error;

    fn try_from(token: CursorToken) -> Result<Self, Self::Error> {
        serde_json::from_str(&token.token).map_err(crate::generic_napi_error)
    }
}
