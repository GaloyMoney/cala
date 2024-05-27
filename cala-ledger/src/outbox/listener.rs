use futures::{FutureExt, Stream};
use tokio::{sync::broadcast, task::JoinHandle};
use tokio_stream::wrappers::{errors::BroadcastStreamRecvError, BroadcastStream};

use std::{collections::BTreeMap, pin::Pin, task::Poll};

use super::{event::*, repo::*};

pub struct OutboxListener {
    repo: OutboxRepo,
    last_returned_sequence: EventSequence,
    latest_known: EventSequence,
    event_receiver: Pin<Box<BroadcastStream<OutboxEvent>>>,
    buffer_size: usize,
    cache: BTreeMap<EventSequence, OutboxEvent>,
    next_page_handle: Option<JoinHandle<Result<Vec<OutboxEvent>, sqlx::Error>>>,
}

impl OutboxListener {
    pub(super) fn new(
        repo: OutboxRepo,
        event_receiver: broadcast::Receiver<OutboxEvent>,
        start_after: EventSequence,
        latest_known: EventSequence,
        buffer: usize,
    ) -> Self {
        Self {
            repo,
            last_returned_sequence: start_after,
            latest_known,
            event_receiver: Box::pin(BroadcastStream::new(event_receiver)),
            cache: BTreeMap::new(),
            next_page_handle: None,
            buffer_size: buffer,
        }
    }

    fn maybe_add_to_cache(&mut self, event: OutboxEvent) {
        self.latest_known = self.latest_known.max(event.sequence);

        if event.sequence > self.last_returned_sequence
            && self.cache.insert(event.sequence, event).is_none()
            && self.cache.len() > self.buffer_size
        {
            self.cache.pop_last();
        }
    }
}

impl Stream for OutboxListener {
    type Item = OutboxEvent;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        // Poll page if present
        if let Some(fetch) = self.next_page_handle.as_mut() {
            match fetch.poll_unpin(cx) {
                Poll::Ready(Ok(Ok(events))) => {
                    for event in events {
                        self.maybe_add_to_cache(event);
                    }
                    self.next_page_handle = None;
                }
                Poll::Ready(_) => {
                    self.next_page_handle = None;
                }
                Poll::Pending => (),
            }
        }
        // Poll as many events as we can come
        loop {
            match self.event_receiver.as_mut().poll_next(cx) {
                Poll::Ready(None) => {
                    if let Some(handle) = self.next_page_handle.take() {
                        handle.abort();
                    }
                    return Poll::Ready(None);
                }
                Poll::Ready(Some(Ok(event))) => {
                    self.maybe_add_to_cache(event);
                }
                Poll::Ready(Some(Err(BroadcastStreamRecvError::Lagged(_)))) => (),
                Poll::Pending => break,
            }
        }

        while let Some((seq, event)) = self.cache.pop_first() {
            if seq <= self.last_returned_sequence {
                continue;
            }
            if seq == self.last_returned_sequence.next() {
                self.last_returned_sequence = seq;
                if let Some(handle) = self.next_page_handle.take() {
                    handle.abort();
                }
                return Poll::Ready(Some(event));
            }
            self.cache.insert(seq, event);
            break;
        }

        if self.next_page_handle.is_none() && self.last_returned_sequence < self.latest_known {
            let repo = self.repo.clone();
            let last_sequence = self.last_returned_sequence;
            let buffer_size = self.buffer_size;
            self.next_page_handle = Some(tokio::spawn(async move {
                repo.load_next_page(last_sequence, buffer_size)
                    .await
                    .map_err(|e| {
                        eprintln!("Error loading next page: {:?}", e);
                        e
                    })
            }));
            return self.poll_next(cx);
        }
        Poll::Pending
    }
}
