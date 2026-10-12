//! Storage ports. Every method is a single-partition lookup or a bounded range scan
//! keyed by `user_id` or `post_id`, so each trait maps directly onto a sharded
//! wide-column store (ScyllaDB) as well as onto SQLite.

use async_trait::async_trait;
use social_core::{Post, PostId, Result, User, UserId, UserStats};
use std::sync::Arc;

#[async_trait]
pub trait UserStore: Send + Sync {
    /// Fails with `Conflict` if the username is taken (case-insensitive).
    async fn create_user(&self, user: &User, pass_hash: &str) -> Result<()>;
    async fn user_by_id(&self, id: UserId) -> Result<Option<User>>;
    async fn user_by_name(&self, username: &str) -> Result<Option<User>>;
    /// The user plus their password hash.
    async fn credentials(&self, username: &str) -> Result<Option<(User, String)>>;
    /// Missing ids are skipped; order is unspecified.
    async fn users_by_ids(&self, ids: &[UserId]) -> Result<Vec<User>>;
    async fn stats(&self, id: UserId) -> Result<UserStats>;
}

#[async_trait]
pub trait SessionStore: Send + Sync {
    async fn create(&self, token_hash: [u8; 32], user: UserId, expires_at: i64) -> Result<()>;
    async fn user_for(&self, token_hash: [u8; 32], now: i64) -> Result<Option<UserId>>;
    async fn delete(&self, token_hash: [u8; 32]) -> Result<()>;
    async fn purge_expired(&self, now: i64) -> Result<u64>;
}

#[async_trait]
pub trait PostStore: Send + Sync {
    async fn insert(&self, post: &Post) -> Result<()>;
    /// Includes tombstoned posts (`deleted = true`); missing ids are skipped.
    async fn get_many(&self, ids: &[PostId]) -> Result<Vec<Post>>;
    /// Tombstones the post if `author` wrote it. Returns whether anything changed.
    async fn soft_delete(&self, id: PostId, author: UserId) -> Result<bool>;
    /// Newest-first ids of live posts by any of `authors`, older than `before`.
    async fn by_authors(&self, authors: &[UserId], before: Option<PostId>, limit: usize) -> Result<Vec<PostId>>;
    /// Newest-first ids of all live posts, older than `before`.
    async fn recent(&self, before: Option<PostId>, limit: usize) -> Result<Vec<PostId>>;
    async fn apply_like_deltas(&self, deltas: &[(PostId, i64)]) -> Result<()>;
}

#[async_trait]
pub trait GraphStore: Send + Sync {
    /// Returns whether a new edge was created.
    async fn follow(&self, follower: UserId, followee: UserId) -> Result<bool>;
    /// Returns whether an edge was removed.
    async fn unfollow(&self, follower: UserId, followee: UserId) -> Result<bool>;
    async fn is_following(&self, follower: UserId, followee: UserId) -> Result<bool>;
    async fn following(&self, user: UserId, limit: usize) -> Result<Vec<UserId>>;
    /// Followers in ascending id order, strictly after `after`.
    async fn followers_page(&self, user: UserId, after: Option<UserId>, limit: usize) -> Result<Vec<UserId>>;
    /// Accounts `user` follows that have at least `min_followers` followers.
    async fn following_with_min_followers(&self, user: UserId, min_followers: i64) -> Result<Vec<UserId>>;
}

#[async_trait]
pub trait LikeStore: Send + Sync {
    /// Returns whether a new like was recorded.
    async fn like(&self, user: UserId, post: PostId) -> Result<bool>;
    /// Returns whether a like was removed.
    async fn unlike(&self, user: UserId, post: PostId) -> Result<bool>;
    /// The subset of `posts` that `user` has liked.
    async fn liked_among(&self, user: UserId, posts: &[PostId]) -> Result<Vec<PostId>>;
}

/// The full set of stores. Backends may differ per store (e.g. sessions in Dragonfly).
#[derive(Clone)]
pub struct Stores {
    pub users: Arc<dyn UserStore>,
    pub sessions: Arc<dyn SessionStore>,
    pub posts: Arc<dyn PostStore>,
    pub graph: Arc<dyn GraphStore>,
    pub likes: Arc<dyn LikeStore>,
}
