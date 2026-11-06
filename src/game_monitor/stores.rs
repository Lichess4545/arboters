mod fixtures;

pub use fixtures::{FixturesStore, PostgresFixturesStore};

#[derive(Debug, thiserror::Error)]
#[error("store query failed")]
pub struct StoreError(#[from] sqlx::Error);
