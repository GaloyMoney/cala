#[must_use]
pub enum Idempotent<T> {
    Executed(T),
    AlreadyApplied,
}

impl<T> Idempotent<T> {
    pub fn was_already_applied(&self) -> bool {
        matches!(self, Idempotent::AlreadyApplied)
    }

    pub fn did_execute(&self) -> bool {
        matches!(self, Idempotent::Executed(_))
    }

    pub fn unwrap(self) -> T {
        match self {
            Idempotent::Executed(t) => t,
            Idempotent::AlreadyApplied => panic!("Idempotent::AlreadyApplied"),
        }
    }
}
