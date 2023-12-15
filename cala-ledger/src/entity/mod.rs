mod error;
mod event;

pub use error::*;
pub use event::*;

pub(crate) struct EntityUpdate<T> {
    pub entity: T,
    pub n_new_events: usize,
}
