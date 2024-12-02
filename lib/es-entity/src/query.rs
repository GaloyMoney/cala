#[derive(Default, std::fmt::Debug, Clone, Copy)]
pub enum ListDirection {
    #[default]
    Ascending,
    Descending,
}

#[derive(std::fmt::Debug, Clone, Copy)]
pub struct Sort<T> {
    pub by: T,
    pub direction: ListDirection,
}

#[derive(Debug)]
pub struct PaginatedQueryArgs<T: std::fmt::Debug> {
    pub first: usize,
    pub after: Option<T>,
}

impl<T: std::fmt::Debug> Default for PaginatedQueryArgs<T> {
    fn default() -> Self {
        Self {
            first: 100,
            after: None,
        }
    }
}

pub struct PaginatedQueryRet<T, C> {
    pub entities: Vec<T>,
    pub has_next_page: bool,
    pub end_cursor: Option<C>,
}
