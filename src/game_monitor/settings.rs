use std::time::Duration;

use secrecy::SecretString;
use serde::Deserialize;
use url::Url;

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LichessSettings {
    pub base_url: Url,
    pub token: Option<SecretString>,
    #[serde(with = "humantime_serde")]
    pub token_check_interval: Duration,
    pub stream: StreamSettings,
}

impl Default for LichessSettings {
    #[expect(clippy::expect_used)]
    fn default() -> Self {
        Self {
            base_url: Url::parse("https://lichess.org").expect("static url is valid"),
            token: None,
            token_check_interval: Duration::from_mins(15),
            stream: StreamSettings::default(),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StreamSettings {
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
            inactivity_timeout: Duration::from_mins(10),
            initial_backoff: Duration::from_secs(1),
            max_backoff: Duration::from_mins(1),
            rate_limit_backoff: Duration::from_mins(1),
        }
    }
}
