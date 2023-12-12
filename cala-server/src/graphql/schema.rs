use async_graphql::{types::connection::*, *};

use super::account::*;

// use timestamp::*;

// use crate::app::CalaApp;

pub struct Query;

#[Object]
impl Query {
    async fn numbers(
        &self,
        after: Option<String>,
        before: Option<String>,
        first: Option<i32>,
        last: Option<i32>,
    ) -> Result<Connection<ID, Account, EmptyFields, EmptyFields>> {
        unimplemented!();
        // query(
        //     after,
        //     before,
        //     first,
        //     last,
        //     |after, before, first, last| async move {
        //         // let mut start = after.map(|after| after + 1).unwrap_or(0);
        //         // let mut end = before.unwrap_or(10000);
        //         // if let Some(first) = first {
        //         //     end = (start + first).min(end);
        //         // }
        //         // if let Some(last) = last {
        //         //     start = if last > end - start { end } else { end - last };
        //         // }
        //         // let mut connection = Connection::new(start > 0, end < 10000);
        //         // connection.edges.extend(
        //         //     (start..end)
        //         //         .into_iter()
        //         //         .map(|n| Edge::with_additional_fields(n, n as i32, EmptyFields)),
        //         // );
        //         Ok::<_, async_graphql::Error>(connection)
        //     },
        // )
        // .await
    }
}

pub struct Mutation;
#[Object]
impl Mutation {
    async fn hello(&self) -> String {
        "Hello, world!".to_string()
    }
}
