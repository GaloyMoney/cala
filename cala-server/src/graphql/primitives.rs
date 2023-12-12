use async_graphql::*;
use serde::{Deserialize, Serialize};

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
#[graphql(remote = "cala_types::primitives::DebitOrCredit")]
pub(super) enum DebitOrCredit {
    Debit,
    Credit,
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
#[graphql(remote = "cala_types::primitives::Status")]
pub(super) enum Status {
    Active,
    Locked,
}

#[derive(Serialize, Deserialize)]
pub struct JSON(serde_json::Value);
scalar!(JSON);
