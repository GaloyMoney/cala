use terrors::OneOf;

#[derive(thiserror::Error, Debug)]
#[error("some error")]
struct SomeError;

#[derive(thiserror::Error, Debug)]
#[error("some other error")]
struct SomeOtherError;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    test()?;
    Ok(())
}

fn test() -> Result<(), OneOf<(SomeOtherError, SomeError)>> {
    Ok(())
}
