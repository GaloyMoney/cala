#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CelType {
    // Builtins
    Map,
    List,
    Int,
    UInt,
    Double,
    String,
    Bytes,
    Bool,
    Null,

    // Abstract
    Date,
    Timestamp,
    Uuid,
    Decimal,
}
