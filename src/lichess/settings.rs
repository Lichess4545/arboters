use std::time::Duration;

use secrecy::SecretString;
use serde::Deserialize;
use url::Url;

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LichessSettings {
    pub base_url: Url,
    pub token: Option<SecretString>,
    pub stream: StreamSettings,
}

impl Default for LichessSettings {
    #[expect(clippy::expect_used)]
    fn default() -> Self {
        Self {
            base_url: Url::parse("https://lichess.org").expect("static url is valid"),
            token: None,
            stream: StreamSettings::default(),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StreamSettings {
    pub max_users_per_connection: usize,
    #[serde(with = "humantime_serde")]
    pub inactivity_timeout: Duration,
    #[serde(with = "humantime_serde")]
    pub initial_backoff: Duration,
    #[serde(with = "humantime_serde")]
    pub max_backoff: Duration,
    #[serde(with = "humantime_serde")]
    pub rate_limit_backoff: Duration,
}

impl Default for StreamSettings {
    fn default() -> Self {
        Self {
            max_users_per_connection: 900,
            inactivity_timeout: Duration::from_mins(10),
            initial_backoff: Duration::from_secs(1),
            max_backoff: Duration::from_mins(1),
            rate_limit_backoff: Duration::from_mins(1),
        }
    }
}
