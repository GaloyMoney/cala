mod config;
mod convert;
mod error;

#[allow(clippy::all)]
pub mod proto {
    tonic::include_proto!("services.outbox.v1");
}

use futures::StreamExt;
use opentelemetry::{
    propagation::{Extractor, TextMapPropagator},
    sdk::propagation::TraceContextPropagator,
};
use proto::{outbox_service_server::OutboxService, *};
use tonic::{transport::Server, Request, Response, Status};
use tracing::instrument;
use tracing_opentelemetry::OpenTelemetrySpanExt;

use super::{EventSequence, Outbox};
pub use config::*;
use error::*;

pub struct OutboxServer {
    outbox: Outbox,
}

#[tonic::async_trait]
impl OutboxService for OutboxServer {
    type SubscribeStream = std::pin::Pin<
        Box<dyn futures::Stream<Item = Result<CalaLedgerEvent, Status>> + Send + Sync + 'static>,
    >;

    #[instrument(name = "cala_ledger.subscribe", skip_all, fields(error, error.level, error.message), err)]
    async fn subscribe(
        &self,
        request: Request<SubscribeRequest>,
    ) -> Result<Response<Self::SubscribeStream>, Status> {
        extract_tracing(&request);

        let SubscribeRequest { after_sequence } = request.into_inner();

        let outbox_listener = self
            .outbox
            .register_listener(after_sequence.map(EventSequence::from))
            .await?;
        Ok(Response::new(Box::pin(
            outbox_listener
                .map(|event| Ok(proto::CalaLedgerEvent::from(event)))
                .fuse(),
        )))
    }
}

pub(crate) async fn start(
    server_config: OutboxServerConfig,
    outbox: Outbox,
) -> Result<(), OutboxServerError> {
    let outbox_service = OutboxServer { outbox };
    Server::builder()
        .add_service(outbox_service_server::OutboxServiceServer::new(
            outbox_service,
        ))
        .serve(([0, 0, 0, 0], server_config.listen_port).into())
        .await?;
    Ok(())
}

pub fn extract_tracing<T>(request: &Request<T>) {
    let propagator = TraceContextPropagator::new();
    let parent_cx = propagator.extract(&RequestContextExtractor(request));
    tracing::Span::current().set_parent(parent_cx)
}

struct RequestContextExtractor<'a, T>(&'a Request<T>);

impl<'a, T> Extractor for RequestContextExtractor<'a, T> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.metadata().get(key).and_then(|s| s.to_str().ok())
    }

    fn keys(&self) -> Vec<&str> {
        self.0
            .metadata()
            .keys()
            .filter_map(|k| {
                if let tonic::metadata::KeyRef::Ascii(key) = k {
                    Some(key.as_str())
                } else {
                    None
                }
            })
            .collect()
    }
}
