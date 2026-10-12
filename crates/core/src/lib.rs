//! Domain types shared by every crate: IDs, models, events, errors and validation.
//! Nothing in here performs I/O.

mod error;
mod event;
mod ids;
mod model;
pub mod validate;

pub use error::{Error, Result};
pub use event::Event;
pub use ids::{id_timestamp_ms, now_ms, IdGen, PostId, UserId, EPOCH_MS};
pub use model::{Post, User, UserStats};
