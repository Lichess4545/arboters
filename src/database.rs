use std::time::Duration;

use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatabaseSettings {
    pub url: SecretString,
    #[serde(default = "DatabaseSettings::default_max_connections")]
    pub max_connections: u32,
    #[serde(
        default = "DatabaseSettings::default_acquire_timeout",
        with = "humantime_serde"
    )]
    pub acquire_timeout: Duration,
}

impl DatabaseSettings {
    const fn default_max_connections() -> u32 {
        5
    }

    const fn default_acquire_timeout() -> Duration {
        Duration::from_secs(5)
    }
}

pub async fn connect(settings: &DatabaseSettings) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(settings.max_connections)
        .acquire_timeout(settings.acquire_timeout)
        .connect(settings.url.expose_secret())
        .await
}
