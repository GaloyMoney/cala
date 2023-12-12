#[derive(Debug)]
pub struct PaginatedQueryArgs<T: std::fmt::Debug> {
    pub after: Option<T>,
    pub before: Option<T>,
    pub first: Option<u32>,
    pub last: Option<u32>,
}

pub struct PaginatedQueryRet<T> {
    pub nodes: Vec<T>,
    pub has_next_page: bool,
    pub has_previous_page: bool,
}
