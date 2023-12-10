use futures::{FutureExt, Stream};
use tokio::{sync::broadcast, task::JoinHandle};
use tokio_stream::wrappers::{errors::BroadcastStreamRecvError, BroadcastStream};

use std::{collections::BTreeMap, pin::Pin, task::Poll};

use super::{error::OutboxError, event::*, repo::*};

pub struct OutboxListener {
    repo: OutboxRepo,
    last_sequence: EventSequence,
    latest_known: EventSequence,
    event_receiver: Pin<Box<BroadcastStream<OutboxEvent>>>,
    buffer_size: usize,
    cache: BTreeMap<EventSequence, OutboxEvent>,
    next_page_handle: Option<JoinHandle<Result<Vec<OutboxEvent>, OutboxError>>>,
}

impl Stream for OutboxListener {
    type Item = OutboxEvent;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        unimplemented!()
    }
}
