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

impl CelType {
    pub(crate) fn package_name(&self) -> &'static str {
        match self {
            CelType::Map => "map",
            CelType::List => "list",
            CelType::Int => "int",
            CelType::UInt => "uint",
            CelType::Double => "double",
            CelType::String => "string",
            CelType::Bytes => "bytes",
            CelType::Bool => "bool",
            CelType::Null => "null",
            CelType::Date => "date",
            CelType::Timestamp => "timestamp",
            CelType::Uuid => "uuid",
            CelType::Decimal => "decimal",
        }
    }
}
