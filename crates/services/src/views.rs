//! API-facing shapes (docs/SPEC.md §4.4).

use serde::Serialize;
use social_core::{MediaId, MediaRef, PostId, User, UserId};

#[derive(Clone, Debug, Serialize)]
pub struct UserView {
    pub id: UserId,
    pub username: String,
    pub created_at: i64,
    /// Empty when unset; clients fall back to the username.
    pub display_name: String,
    pub bio: String,
}

impl From<User> for UserView {
    fn from(u: User) -> Self {
        Self { id: u.id, username: u.username, created_at: u.created_at, display_name: u.display_name, bio: u.bio }
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
    /// Username of the parent post's author, when this is a reply.
    pub replying_to: Option<String>,
    pub created_at: i64,
    pub like_count: i64,
    pub reply_count: i64,
    pub liked: bool,
    pub media: Vec<MediaView>,
}

/// An image as clients see it: `url` is where to fetch the bytes.
#[derive(Clone, Debug, Serialize)]
pub struct MediaView {
    pub id: MediaId,
    pub url: String,
    pub content_type: String,
    pub width: u32,
    pub height: u32,
    pub alt: String,
}

impl From<MediaRef> for MediaView {
    fn from(m: MediaRef) -> Self {
        Self { url: media_url(m.id), id: m.id, content_type: m.content_type, width: m.width, height: m.height, alt: m.alt }
    }
}

/// Public path of an image's bytes (served by the gateway).
pub fn media_url(id: MediaId) -> String {
    format!("/api/v1/media/{id}")
}

#[derive(Debug, Serialize)]
pub struct Page<T, C = PostId> {
    pub items: Vec<T>,
    /// Cursor for the next page (`before` for timelines, `after` for replies and user
    /// lists); `None` at the end.
    pub next: Option<C>,
}
