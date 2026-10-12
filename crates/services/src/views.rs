//! API-facing shapes (docs/SPEC.md §4.4).

use serde::Serialize;
use social_core::{PostId, User, UserId};

#[derive(Clone, Debug, Serialize)]
pub struct UserView {
    pub id: UserId,
    pub username: String,
    pub created_at: i64,
}

impl From<User> for UserView {
    fn from(u: User) -> Self {
        Self { id: u.id, username: u.username, created_at: u.created_at }
    }
}

#[derive(Debug, Serialize)]
pub struct ProfileView {
    #[serde(flatten)]
    pub user: UserView,
    pub followers: i64,
    pub following: i64,
    pub posts: i64,
    pub followed_by_me: bool,
}

#[derive(Debug, Serialize)]
pub struct PostView {
    pub id: PostId,
    pub author: UserView,
    pub body: String,
    pub reply_to: Option<PostId>,
    pub created_at: i64,
    pub like_count: i64,
    pub liked: bool,
}

#[derive(Debug, Serialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    /// Pass as `before` to fetch the next page; `None` at the end.
    pub next: Option<PostId>,
}
