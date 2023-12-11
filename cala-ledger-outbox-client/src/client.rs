#[allow(clippy::all)]
mod proto {
    tonic::include_proto!("services.outbox.v1");
}

use tracing::instrument;

use super::{config::*, error::*};

type ProtoClient = proto::outbox_service_client::OutboxServiceClient<tonic::transport::Channel>;

pub struct CalaLedgerOutboxClient {
    config: CalaLedgerOutboxClientConfig,
    proto_client: ProtoClient,
}
impl CalaLedgerOutboxClient {
    pub async fn connect(
        config: CalaLedgerOutboxClientConfig,
    ) -> Result<Self, CalaLedgerOutboxClientError> {
        let proto_client = ProtoClient::connect(config.url.clone()).await?;

        Ok(Self {
            config,
            proto_client,
        })
    }

    // pub async fn subscribe(
    //     &self,
    //     after_sequence: Option<u64>,
    // ) -> Result<impl Stream<proto::, CalaLedgerOutboxClientError> {
    //     // let request = tonic::Request::new(proto::SubscribeRequest { after_sequence });

    //     // let mut stream = self.connect().await?.subscribe(request).await?.into_inner();
    //     unimplemented!();
    // }
}
