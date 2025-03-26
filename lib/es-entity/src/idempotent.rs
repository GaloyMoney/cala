#[must_use]
pub enum Idempotent<T> {
    Executed(T),
    Ignored,
}

impl<T> Idempotent<T> {
    pub fn was_ignored(&self) -> bool {
        matches!(self, Idempotent::Ignored)
    }

    pub fn did_execute(&self) -> bool {
        matches!(self, Idempotent::Executed(_))
    }

    pub fn unwrap(self) -> T {
        match self {
            Idempotent::Executed(t) => t,
            Idempotent::Ignored => panic!("Idempotent::Ignored"),
        }
    }
}
