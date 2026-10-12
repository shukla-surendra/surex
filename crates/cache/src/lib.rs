//! Home-timeline cache port and its in-process implementation. The cluster
//! implementation (Dragonfly) maps `push` to `LPUSHX`-style writes on capped lists.

use async_trait::async_trait;
use moka::sync::Cache;
use social_core::{PostId, Result, UserId};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

pub struct TimelinePage {
    /// Newest first.
    pub ids: Vec<PostId>,
    /// True when the cache holds the user's whole timeline below the cursor; false
    /// when older entries were dropped by the cap and must come from storage.
    pub complete: bool,
}

#[async_trait]
pub trait TimelineCache: Send + Sync {
    /// `None` on a cache miss.
    async fn range(&self, user: UserId, before: Option<PostId>, limit: usize) -> Result<Option<TimelinePage>>;
    /// Installs a timeline rebuilt from storage (newest first, `cap` entries max).
    async fn seed(&self, user: UserId, ids: Vec<PostId>) -> Result<()>;
    /// Adds `post` to each listed user's timeline **only if it is cached**, so inactive
    /// users cost nothing.
    async fn push(&self, users: &[UserId], post: PostId) -> Result<()>;
    /// Adds several posts to one user's timeline, only if it is cached.
    async fn merge(&self, user: UserId, posts: &[PostId]) -> Result<()>;
    async fn invalidate(&self, user: UserId) -> Result<()>;
}

struct Timeline {
    ids: VecDeque<PostId>,
    truncated: bool,
}

impl Timeline {
    fn insert(&mut self, post: PostId, cap: usize) {
        // Kept sorted newest-first; concurrent fan-out can deliver out of order.
        let pos = self.ids.partition_point(|&x| x > post);
        if self.ids.get(pos) == Some(&post) {
            return;
        }
        if pos >= cap {
            self.truncated = true;
            return;
        }
        self.ids.insert(pos, post);
        if self.ids.len() > cap {
            self.ids.pop_back();
            self.truncated = true;
        }
    }
}

/// Bounded in-process cache with TinyLFU admission/eviction.
pub struct InProcTimelineCache {
    timelines: Cache<UserId, Arc<Mutex<Timeline>>>,
    cap: usize,
}

impl InProcTimelineCache {
    pub fn new(max_users: u64, cap: usize) -> Self {
        Self { timelines: Cache::new(max_users), cap }
    }
}

#[async_trait]
impl TimelineCache for InProcTimelineCache {
    async fn range(&self, user: UserId, before: Option<PostId>, limit: usize) -> Result<Option<TimelinePage>> {
        let Some(t) = self.timelines.get(&user) else { return Ok(None) };
        let t = t.lock().unwrap();
        let start = before.map_or(0, |b| t.ids.partition_point(|&x| x >= b));
        let ids: Vec<PostId> = t.ids.iter().skip(start).take(limit).copied().collect();
        Ok(Some(TimelinePage { ids, complete: !t.truncated }))
    }

    async fn seed(&self, user: UserId, mut ids: Vec<PostId>) -> Result<()> {
        let truncated = ids.len() >= self.cap;
        ids.truncate(self.cap);
        self.timelines.insert(user, Arc::new(Mutex::new(Timeline { ids: ids.into(), truncated })));
        Ok(())
    }

    async fn push(&self, users: &[UserId], post: PostId) -> Result<()> {
        for user in users {
            if let Some(t) = self.timelines.get(user) {
                t.lock().unwrap().insert(post, self.cap);
            }
        }
        Ok(())
    }

    async fn merge(&self, user: UserId, posts: &[PostId]) -> Result<()> {
        if let Some(t) = self.timelines.get(&user) {
            let mut t = t.lock().unwrap();
            for &p in posts {
                t.insert(p, self.cap);
            }
        }
        Ok(())
    }

    async fn invalidate(&self, user: UserId) -> Result<()> {
        self.timelines.invalidate(&user);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(v: &[i64]) -> Vec<PostId> {
        v.iter().copied().map(PostId).collect()
    }

    #[tokio::test]
    async fn push_only_affects_cached_users_and_keeps_order() {
        let c = InProcTimelineCache::new(100, 3);
        let (a, b) = (UserId(1), UserId(2));
        c.seed(a, vec![]).await.unwrap();
        for p in [5, 9, 7, 9] {
            c.push(&[a, b], PostId(p)).await.unwrap();
        }
        let page = c.range(a, None, 10).await.unwrap().unwrap();
        assert_eq!(page.ids, ids(&[9, 7, 5]));
        assert!(page.complete);
        assert!(c.range(b, None, 10).await.unwrap().is_none(), "uncached user must stay uncached");

        c.push(&[a], PostId(8)).await.unwrap();
        let page = c.range(a, None, 10).await.unwrap().unwrap();
        assert_eq!(page.ids, ids(&[9, 8, 7]));
        assert!(!page.complete, "cap evicted an entry");
        assert_eq!(c.range(a, Some(PostId(8)), 10).await.unwrap().unwrap().ids, ids(&[7]));
    }
}
