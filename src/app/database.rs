mod config;

use secrecy::ExposeSecret;
use sqlx::PgPool;

pub use config::Config;

pub async fn connect(config: &Config) -> Result<PgPool, sqlx::Error> {
    PgPool::connect(config.url.expose_secret()).await
}
