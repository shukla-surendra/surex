//! Event bus port and its in-process implementation. A Redpanda/Kafka implementation
//! bridges each consumer group's partitions into the same `mpsc::Receiver` interface.

use async_trait::async_trait;
use social_core::{Event, Result};
use std::sync::RwLock;
use tokio::sync::mpsc;

/// Per-group buffer. A full buffer applies backpressure to publishers.
const GROUP_CAPACITY: usize = 65_536;

#[async_trait]
pub trait EventBus: Send + Sync {
    async fn publish(&self, event: Event) -> Result<()>;
    /// Registers a consumer group. Each group receives every event published after it
    /// subscribed.
    fn subscribe(&self, group: &str) -> mpsc::Receiver<Event>;
    /// Stops delivery. Consumers drain their buffers and then see the end of the stream.
    fn close(&self);
}

#[derive(Default)]
pub struct InProcBus {
    groups: RwLock<Vec<(String, mpsc::Sender<Event>)>>,
}

impl InProcBus {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl EventBus for InProcBus {
    async fn publish(&self, event: Event) -> Result<()> {
        let senders: Vec<_> = self.groups.read().unwrap().iter().map(|(g, s)| (g.clone(), s.clone())).collect();
        for (group, tx) in senders {
            if tx.send(event.clone()).await.is_err() {
                tracing::warn!(%group, ?event, "consumer group gone; event dropped");
            }
        }
        Ok(())
    }

    fn subscribe(&self, group: &str) -> mpsc::Receiver<Event> {
        let (tx, rx) = mpsc::channel(GROUP_CAPACITY);
        self.groups.write().unwrap().push((group.to_owned(), tx));
        rx
    }

    fn close(&self) {
        self.groups.write().unwrap().clear();
    }
}
