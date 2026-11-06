mod fixtures;
mod published_events;

pub use fixtures::{FixturesStore, PostgresFixturesStore};
pub use published_events::{PostgresPublishedEventsStore, PublishedEventsStore, Recorded};

#[derive(Debug, thiserror::Error)]
#[error("store query failed")]
pub struct StoreError(#[from] sqlx::Error);
