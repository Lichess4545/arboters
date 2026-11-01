use std::time::Duration;

use secrecy::SecretString;
use serde::Deserialize;
use url::Url;

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub base_url: Url,
    pub token: Option<SecretString>,
    pub stream: StreamConfig,
}

impl Default for Config {
    #[expect(clippy::expect_used)]
    fn default() -> Self {
        Self {
            base_url: Url::parse("https://lichess.org").expect("static url is valid"),
            token: None,
            stream: StreamConfig::default(),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StreamConfig {
    #[serde(with = "humantime_serde")]
    pub inactivity_timeout: Duration,
    #[serde(with = "humantime_serde")]
    pub initial_backoff: Duration,
    #[serde(with = "humantime_serde")]
    pub max_backoff: Duration,
    #[serde(with = "humantime_serde")]
    pub rate_limit_backoff: Duration,
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            inactivity_timeout: Duration::from_mins(10),
            initial_backoff: Duration::from_secs(1),
            max_backoff: Duration::from_mins(1),
            rate_limit_backoff: Duration::from_mins(1),
        }
    }
}
