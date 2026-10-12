use crate::{MediaId, PostId, UserId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug)]
pub struct User {
    pub id: UserId,
    pub username: String,
    pub created_at: i64,
    /// Free-form name shown next to the handle; empty when unset.
    pub display_name: String,
    pub bio: String,
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
    /// Live replies; maintained on reply insert and delete.
    pub reply_count: i64,
    /// Attached images, in display order (at most `validate::MAX_MEDIA`).
    pub media: Vec<MediaRef>,
    /// Soft-delete tombstone: cached timelines may still reference the id.
    pub deleted: bool,
}

/// An image attached to a post. Stored with the post (as JSON), so rendering a post
/// needs no extra lookup.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaRef {
    pub id: MediaId,
    pub content_type: String,
    pub width: u32,
    pub height: u32,
    /// Alt text for screen readers; may be empty.
    #[serde(default)]
    pub alt: String,
}

/// An uploaded image's metadata, kept next to its bytes in the media store.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaMeta {
    pub id: MediaId,
    pub owner: UserId,
    pub content_type: String,
    pub width: u32,
    pub height: u32,
    pub bytes: u64,
}
