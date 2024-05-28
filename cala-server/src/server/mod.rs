mod config;

use async_graphql::*;
use async_graphql_axum::{GraphQLRequest, GraphQLResponse};
use axum::{routing::get, Extension, Router};
use axum_extra::headers::HeaderMap;
use cala_ledger::{AtomicOperation, CalaLedger};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::{app::CalaApp, extension::MutationExtensionMarker, graphql};

pub use config::*;

pub async fn run<M: MutationExtensionMarker>(
    config: ServerConfig,
    app: CalaApp,
) -> anyhow::Result<()> {
    let ledger = app.ledger().clone();
    let schema = graphql::schema::<M>(Some(app));

    let app = Router::new()
        .route(
            "/graphql",
            get(playground).post(axum::routing::post(graphql_handler::<M>)),
        )
        .layer(Extension(schema))
        .layer(Extension(ledger));

    println!("Starting graphql server on port {}", config.port);
    let listener =
        tokio::net::TcpListener::bind(&std::net::SocketAddr::from(([0, 0, 0, 0], config.port)))
            .await?;
    axum::serve(listener, app.into_make_service()).await?;
    Ok(())
}

pub async fn graphql_handler<M: MutationExtensionMarker>(
    headers: HeaderMap,
    schema: Extension<Schema<graphql::Query, graphql::CoreMutation<M>, EmptySubscription>>,
    Extension(ledger): Extension<CalaLedger>,
    req: GraphQLRequest,
) -> GraphQLResponse {
    cala_tracing::http::extract_tracing(&headers);
    let mut req = req.into_inner();
    let op = match maybe_init_atomic_operation(&mut req, &ledger).await {
        Err(e) => {
            return async_graphql::Response::from_errors(vec![async_graphql::ServerError::new(
                e.to_string(),
                None,
            )])
            .into();
        }
        Ok(op) => op,
    };
    if let Some(ref op) = op {
        req = req.data(Arc::clone(op));
    }
    let mut res = schema.execute(req).await;
    if let Some(op) = op {
        if res.errors.is_empty() {
            if let Err(e) = Arc::into_inner(op)
                .expect("Arc::into_inner")
                .into_inner()
                .commit()
                .await
            {
                res.errors
                    .push(async_graphql::ServerError::new(e.to_string(), None))
            }
        }
    }
    res.into()
}

async fn playground() -> impl axum::response::IntoResponse {
    axum::response::Html(async_graphql::http::playground_source(
        async_graphql::http::GraphQLPlaygroundConfig::new("/graphql"),
    ))
}

async fn maybe_init_atomic_operation<'a>(
    req: &mut async_graphql::Request,
    ledger: &CalaLedger,
) -> Result<Option<Arc<Mutex<AtomicOperation<'a>>>>, cala_ledger::error::LedgerError> {
    let operation_name = req
        .operation_name
        .as_ref()
        .map(|n| async_graphql::Name::new(n.clone()));
    if let Ok(query) = req.parsed_query() {
        let is_mutation = match (&query.operations, operation_name) {
            (async_graphql::parser::types::DocumentOperations::Single(op), _)
                if op.node.ty == async_graphql::parser::types::OperationType::Mutation =>
            {
                true
            }
            (async_graphql::parser::types::DocumentOperations::Multiple(ops), _)
                if ops.len() == 1 =>
            {
                if ops.values().next().expect("ops.next").node.ty
                    == async_graphql::parser::types::OperationType::Mutation
                {
                    true
                } else {
                    false
                }
            }
            (async_graphql::parser::types::DocumentOperations::Multiple(ops), Some(name))
                if ops.get(&name).is_some() =>
            {
                if ops.get(&name).expect("ops.get").node.ty
                    == async_graphql::parser::types::OperationType::Mutation
                {
                    true
                } else {
                    false
                }
            }
            _ => false,
        };
        if is_mutation {
            return Ok(Some(Arc::new(Mutex::new(ledger.begin_operation().await?))));
        }
    }
    Ok(None)
}
