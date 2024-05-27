mod config;

use async_graphql::*;
use async_graphql_axum::{GraphQLRequest, GraphQLResponse};
use axum::{routing::get, Extension, Router};
use axum_extra::headers::HeaderMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::{app::CalaApp, extension::MutationExtensionMarker, graphql};

pub use config::*;

pub async fn run<M: MutationExtensionMarker>(
    config: ServerConfig,
    app: CalaApp,
) -> anyhow::Result<()> {
    let schema = graphql::schema::<M>(Some(app.clone()));

    let app = Router::new()
        .route(
            "/graphql",
            get(playground).post(axum::routing::post(graphql_handler::<M>)),
        )
        .layer(Extension(schema))
        .layer(Extension(app));

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
    Extension(app): Extension<CalaApp>,
    req: GraphQLRequest,
) -> GraphQLResponse {
    cala_tracing::http::extract_tracing(&headers);
    let mut req = req.into_inner();
    let mut op = None;
    if let Ok(query) = req.parsed_query() {
        if query
            .operations
            .iter()
            .any(|(_, o)| o.node.ty == async_graphql::parser::types::OperationType::Mutation)
        {
            let operation = Arc::new(Mutex::new(match app.ledger().begin_operation().await {
                Err(e) => {
                    return async_graphql::Response::from_errors(vec![
                        async_graphql::ServerError::new(e.to_string(), None),
                    ])
                    .into();
                }
                Ok(op) => op,
            }));
            req = req.data(Arc::clone(&operation));
            op = Some(operation);
        }
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
