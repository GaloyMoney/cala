#[allow(clippy::all)]
pub(crate) mod proto {
    tonic::include_proto!("services.outbox.v1");
}

use futures::StreamExt;
use tracing::instrument;

use super::{config::*, error::*};
use cala_types::outbox::*;

type ProtoClient = proto::outbox_service_client::OutboxServiceClient<tonic::transport::Channel>;

pub struct CalaLedgerOutboxClient {
    _config: CalaLedgerOutboxClientConfig,
    proto_client: ProtoClient,
}
impl CalaLedgerOutboxClient {
    pub async fn connect(
        config: CalaLedgerOutboxClientConfig,
    ) -> Result<Self, CalaLedgerOutboxClientError> {
        let proto_client = ProtoClient::connect(config.url.clone()).await?;

        Ok(Self {
            _config: config,
            proto_client,
        })
    }

    #[instrument(name = "cala_ledger.outbox_client.subscribe", skip(self))]
    pub async fn subscribe(
        &mut self,
        after_sequence: Option<u64>,
    ) -> Result<
        impl futures::Stream<Item = Result<OutboxEvent, CalaLedgerOutboxClientError>>,
        CalaLedgerOutboxClientError,
    > {
        let request = tonic::Request::new(proto::SubscribeRequest { after_sequence });
        let stream = self.proto_client.subscribe(request).await?.into_inner();
        Ok(stream.map(|e| {
            e.map_err(CalaLedgerOutboxClientError::from)
                .and_then(OutboxEvent::try_from)
        }))
    }
}
