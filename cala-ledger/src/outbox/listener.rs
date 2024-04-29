// use futures::{FutureExt, Stream};
// use tokio::{sync::broadcast, task::JoinHandle};
// use tokio_stream::wrappers::{errors::BroadcastStreamRecvError, BroadcastStream};
use futures::Stream;
use tokio::task::JoinHandle;
use tokio_stream::wrappers::BroadcastStream;

use std::{collections::BTreeMap, pin::Pin, task::Poll};

use super::{error::OutboxError, event::*, repo::*};

pub struct OutboxListener {
    _repo: OutboxRepo,
    _last_sequence: EventSequence,
    _latest_known: EventSequence,
    _event_receiver: Pin<Box<BroadcastStream<OutboxEvent>>>,
    _buffer_size: usize,
    _cache: BTreeMap<EventSequence, OutboxEvent>,
    _next_page_handle: Option<JoinHandle<Result<Vec<OutboxEvent>, OutboxError>>>,
}

impl Stream for OutboxListener {
    type Item = OutboxEvent;

    fn poll_next(
        self: Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        unimplemented!()
    }
}
