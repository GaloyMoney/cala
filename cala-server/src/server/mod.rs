mod config;

use async_graphql::*;
use async_graphql_axum::{GraphQLRequest, GraphQLResponse};
use axum::{routing::get, Extension, Router};
use axum_extra::headers::HeaderMap;
use cala_ledger::{CalaLedger, LedgerOperation};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::instrument;

use crate::{app::CalaApp, extension::*, graphql};

pub use config::*;

pub async fn run<Q: QueryExtensionMarker, M: MutationExtensionMarker>(
    config: ServerConfig,
    app: CalaApp,
) -> anyhow::Result<()> {
    let ledger = app.ledger().clone();
    let schema = graphql::schema::<Q, M>(Some(app));

    let app = Router::new()
        .route(
            "/graphql",
            get(playground).post(axum::routing::post(graphql_handler::<Q, M>)),
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

#[instrument(name = "cala_server.graphql", skip_all, fields(error, error.level, error.message))]
pub async fn graphql_handler<Q: QueryExtensionMarker, M: MutationExtensionMarker>(
    headers: HeaderMap,
    schema: Extension<Schema<graphql::CoreQuery<Q>, graphql::CoreMutation<M>, EmptySubscription>>,
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

async fn maybe_init_atomic_operation(
    req: &mut async_graphql::Request,
    ledger: &CalaLedger,
) -> Result<Option<Arc<Mutex<LedgerOperation<'static>>>>, cala_ledger::error::LedgerError> {
    use async_graphql::parser::types::*;

    let operation_name = req
        .operation_name
        .as_ref()
        .map(|n| async_graphql::Name::new(n.clone()));
    if let Ok(query) = req.parsed_query() {
        let is_mutation = match (&query.operations, operation_name) {
            (DocumentOperations::Single(op), _) => op.node.ty == OperationType::Mutation,
            (DocumentOperations::Multiple(ops), _) if ops.len() == 1 => {
                ops.values().next().expect("ops.next").node.ty == OperationType::Mutation
            }
            (DocumentOperations::Multiple(ops), Some(name)) if ops.get(&name).is_some() => {
                ops.get(&name).expect("ops.get").node.ty == OperationType::Mutation
            }
            _ => false,
        };
        if is_mutation {
            return Ok(Some(Arc::new(Mutex::new(ledger.begin_operation().await?))));
        }
    }
    Ok(None)
}
