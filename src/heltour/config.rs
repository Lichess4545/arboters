use std::time::Duration;

use secrecy::SecretString;
use serde::Deserialize;
use url::Url;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub base_url: Url,
    pub token: SecretString,
    #[serde(default)]
    pub retry: RetryConfig,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RetryConfig {
    pub retries: usize,
    #[serde(with = "humantime_serde")]
    pub initial_backoff: Duration,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            retries: 5,
            initial_backoff: Duration::from_secs(1),
        }
    }
}
