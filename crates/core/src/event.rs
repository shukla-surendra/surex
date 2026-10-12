use crate::{PostId, UserId};
use serde::{Deserialize, Serialize};

/// State changes published after the write commits (docs/SPEC.md §6).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    PostCreated { post_id: PostId, author_id: UserId },
    PostDeleted { post_id: PostId, author_id: UserId },
    Followed { follower: UserId, followee: UserId },
    Unfollowed { follower: UserId, followee: UserId },
    Liked { post_id: PostId, user_id: UserId },
    Unliked { post_id: PostId, user_id: UserId },
}
