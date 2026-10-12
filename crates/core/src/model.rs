use crate::{PostId, UserId};

#[derive(Clone, Debug)]
pub struct User {
    pub id: UserId,
    pub username: String,
    pub created_at: i64,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct UserStats {
    pub followers: i64,
    pub following: i64,
    pub posts: i64,
}

#[derive(Clone, Debug)]
pub struct Post {
    pub id: PostId,
    pub author_id: UserId,
    pub body: String,
    pub reply_to: Option<PostId>,
    pub created_at: i64,
    pub like_count: i64,
    /// Soft-delete tombstone: cached timelines may still reference the id.
    pub deleted: bool,
}
