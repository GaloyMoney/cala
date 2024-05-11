mod config;

use async_graphql::*;
use async_graphql_axum::{GraphQLRequest, GraphQLResponse};
use axum::{routing::get, Extension, Router};
use axum_extra::headers::HeaderMap;

use cala_extension::*;

use crate::{app::CalaApp, graphql};

pub use config::*;

pub async fn run<M: MutationExtension>(
    config: ServerConfig,
    app: CalaApp,
    _extensions: Vec<Box<dyn CalaExtension>>,
) -> anyhow::Result<()> {
    let schema = graphql::schema::<M>(Some(app.clone()));

    let app = Router::new()
        .route(
            "/graphql",
            get(playground).post(axum::routing::post(graphql_handler)),
        )
        .layer(Extension(schema));

    println!("Starting graphql server on port {}", config.port);
    let listener =
        tokio::net::TcpListener::bind(&std::net::SocketAddr::from(([0, 0, 0, 0], config.port)))
            .await?;
    axum::serve(listener, app.into_make_service()).await?;
    Ok(())
}

pub async fn graphql_handler(
    headers: HeaderMap,
    schema: Extension<Schema<graphql::Query, graphql::CoreMutations, EmptySubscription>>,
    req: GraphQLRequest,
) -> GraphQLResponse {
    cala_tracing::http::extract_tracing(&headers);
    let req = req.into_inner();
    schema.execute(req).await.into()
}

async fn playground() -> impl axum::response::IntoResponse {
    axum::response::Html(async_graphql::http::playground_source(
        async_graphql::http::GraphQLPlaygroundConfig::new("/graphql"),
    ))
}
