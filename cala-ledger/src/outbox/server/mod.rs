#![allow(clippy::blocks_in_conditions)]
mod config;
mod convert;
pub mod error;

#[allow(clippy::all)]
pub mod proto {
    tonic::include_proto!("services.outbox.v1");
}

use futures::StreamExt;
use proto::{outbox_service_server::OutboxService, *};
use tonic::{transport::Server, Request, Response, Status};
use tracing::instrument;

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
        cala_tracing::grpc::extract_tracing(&request);

        let SubscribeRequest { after_sequence } = request.into_inner();

        let outbox_listener = self
            .outbox
            .register_listener(after_sequence.map(EventSequence::from))
            .await
            .map_err(|e| tonic::Status::internal(e.to_string()))?;
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
