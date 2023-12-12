use serde::{Deserialize, Serialize};

use super::primitives::AccountId;

#[derive(Debug)]
pub struct PaginatedQueryArgs<T: std::fmt::Debug> {
    pub after: Option<T>,
    pub first: usize,
}

pub struct PaginatedQueryRet<T, C> {
    pub nodes: Vec<T>,
    pub has_next_page: bool,
    pub end_cursor: Option<C>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AccountByNameCursor {
    pub name: String,
    pub id: AccountId,
}
