use darling::FromMeta;

#[derive(Debug, Default, Clone, Copy, FromMeta, PartialEq)]
pub enum DeleteOption {
    #[default]
    No,
    Soft,
}

impl DeleteOption {
    pub fn include_deletion_fn_postfix(&self) -> &'static str {
        match self {
            DeleteOption::Soft => "_include_deleted",
            DeleteOption::No => "",
        }
    }

    pub fn not_deleted_condition(&self) -> &'static str {
        match self {
            DeleteOption::Soft => " AND deleted = FALSE",
            DeleteOption::No => "",
        }
    }
}

impl std::str::FromStr for DeleteOption {
    type Err = darling::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "no" => Ok(DeleteOption::No),
            "soft" => Ok(DeleteOption::Soft),
            _ => Err(darling::Error::unknown_value(s)),
        }
    }
}
