use async_graphql::*;

// use timestamp::*;

// use crate::app::CalaApp;

pub struct Query;

#[Object]
impl Query {
    async fn hello(&self) -> String {
        "Hello, world!".to_string()
    }
}

pub struct Mutation;
#[Object]
impl Mutation {
    async fn hello(&self) -> String {
        "Hello, world!".to_string()
    }
}
