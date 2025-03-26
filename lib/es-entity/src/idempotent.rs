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

pub trait FromIdempotentIgnored {
    fn from_ignored() -> Self;
}

impl<T> FromIdempotentIgnored for Idempotent<T> {
    fn from_ignored() -> Self {
        Idempotent::Ignored
    }
}

impl<T, E> FromIdempotentIgnored for Result<Idempotent<T>, E> {
    fn from_ignored() -> Self {
        Ok(Idempotent::Ignored)
    }
}
