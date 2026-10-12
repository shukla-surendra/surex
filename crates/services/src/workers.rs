//! Event consumers (docs/SPEC.md §6). Each runs until the bus closes, then drains.

use crate::App;
use social_core::{Event, PostId, Result};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Semaphore};
use tokio::task::JoinHandle;

const FANOUT_CONCURRENCY: usize = 64;
const COUNTER_FLUSH_EVERY: Duration = Duration::from_millis(500);
const COUNTER_FLUSH_POSTS: usize = 1_000;
/// Recent posts merged into a timeline when its owner follows someone new.
const FOLLOW_BACKFILL: usize = 50;

#[derive(Clone, Copy, Debug)]
pub struct WorkerSet {
    pub fanout: bool,
    pub counters: bool,
}

impl WorkerSet {
    pub fn all() -> Self {
        Self { fanout: true, counters: true }
    }
}

pub struct Workers(Vec<JoinHandle<()>>);

impl Workers {
    /// Waits for every worker to drain. Call after `EventBus::close`.
    pub async fn join(self) {
        for h in self.0 {
            let _ = h.await;
        }
    }
}

/// Subscribes and starts the selected workers. Call before serving traffic so no
/// event is published before its consumers exist.
pub fn spawn_workers(app: &App, set: WorkerSet) -> Workers {
    let mut handles = Vec::new();
    if set.fanout {
        handles.push(tokio::spawn(fanout_loop(app.clone(), app.bus.subscribe("fanout"))));
    }
    if set.counters {
        handles.push(tokio::spawn(counter_loop(app.clone(), app.bus.subscribe("counters"))));
    }
    Workers(handles)
}

async fn fanout_loop(app: App, mut rx: mpsc::Receiver<Event>) {
    let permits = Arc::new(Semaphore::new(FANOUT_CONCURRENCY));
    while let Some(event) = rx.recv().await {
        let permit = permits.clone().acquire_owned().await.expect("semaphore never closed");
        let app = app.clone();
        tokio::spawn(async move {
            if let Err(e) = fanout(&app, &event).await {
                tracing::warn!(error = %e, ?event, "fan-out failed");
            }
            drop(permit);
        });
    }
    // Wait for in-flight fan-outs before reporting drained.
    let _ = permits.acquire_many(FANOUT_CONCURRENCY as u32).await;
}

async fn fanout(app: &App, event: &Event) -> Result<()> {
    match *event {
        Event::PostCreated { post_id, author_id } => {
            app.cache.push(&[author_id], post_id).await?;
            if app.stores.users.stats(author_id).await?.followers >= app.cfg.celebrity_threshold {
                return Ok(()); // Fanned out on read instead.
            }
            let mut after = None;
            loop {
                let batch = app.stores.graph.followers_page(author_id, after, app.cfg.fanout_batch).await?;
                if batch.is_empty() {
                    break;
                }
                app.cache.push(&batch, post_id).await?;
                if batch.len() < app.cfg.fanout_batch {
                    break;
                }
                after = batch.last().copied();
            }
        }
        Event::Followed { follower, followee } => {
            let recent = app.stores.posts.by_authors(&[followee], None, FOLLOW_BACKFILL).await?;
            app.cache.merge(follower, &recent).await?;
        }
        Event::Unfollowed { follower, .. } => app.cache.invalidate(follower).await?,
        // Deleted posts are filtered when timelines are hydrated.
        Event::PostDeleted { .. } | Event::Liked { .. } | Event::Unliked { .. } => {}
    }
    Ok(())
}

async fn counter_loop(app: App, mut rx: mpsc::Receiver<Event>) {
    let mut pending: HashMap<PostId, i64> = HashMap::new();
    let mut tick = tokio::time::interval(COUNTER_FLUSH_EVERY);
    loop {
        tokio::select! {
            event = rx.recv() => match event {
                Some(Event::Liked { post_id, .. }) => *pending.entry(post_id).or_default() += 1,
                Some(Event::Unliked { post_id, .. }) => *pending.entry(post_id).or_default() -= 1,
                Some(_) => {}
                None => break,
            },
            _ = tick.tick() => flush(&app, &mut pending).await,
        }
        if pending.len() >= COUNTER_FLUSH_POSTS {
            flush(&app, &mut pending).await;
        }
    }
    flush(&app, &mut pending).await;
}

async fn flush(app: &App, pending: &mut HashMap<PostId, i64>) {
    let deltas: Vec<(PostId, i64)> = pending.iter().filter(|(_, d)| **d != 0).map(|(p, d)| (*p, *d)).collect();
    if deltas.is_empty() {
        pending.clear();
        return;
    }
    match app.stores.posts.apply_like_deltas(&deltas).await {
        Ok(()) => pending.clear(),
        // Keep the deltas and retry on the next tick.
        Err(e) => tracing::warn!(error = %e, posts = deltas.len(), "like counter flush failed"),
    }
}
