use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use sqlx::PgPool;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatabaseSettings {
    pub url: SecretString,
}

pub async fn connect(settings: &DatabaseSettings) -> Result<PgPool, sqlx::Error> {
    PgPool::connect(settings.url.expose_secret()).await
}
