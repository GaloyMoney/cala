use cala_extension::CalaExtension;

pub struct TestExtension {}

impl CalaExtension for TestExtension {}

pub struct Query;

use async_graphql::*;

#[Object]
impl Query {
    async fn hello(&self) -> String {
        "world".to_string()
    }
}

struct TestExtensionType;

#[Object]
impl TestExtensionType {
    async fn hello(&self) -> String {
        "world".to_string()
    }
}
