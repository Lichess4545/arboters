use std::time::Duration;

use secrecy::SecretString;
use serde::Deserialize;
use url::Url;

use super::UserId;

const ANONYMOUS_USERS_PER_CONNECTION: usize = 300;
const TOKEN_USERS_PER_CONNECTION: usize = 500;
const LICHESS4545_USERS_PER_CONNECTION: usize = 900;
const LICHESS4545: &str = "lichess4545";

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub base_url: Url,
    pub token: Option<SecretString>,
    pub stream: StreamConfig,
}

impl Config {
    #[must_use]
    pub fn users_per_connection(&self, token_owner: Option<&UserId>) -> usize {
        match (&self.token, token_owner) {
            (None, _) => ANONYMOUS_USERS_PER_CONNECTION,
            (Some(_), Some(owner)) if owner.as_str() == LICHESS4545 => {
                LICHESS4545_USERS_PER_CONNECTION
            }
            (Some(_), _) => TOKEN_USERS_PER_CONNECTION,
        }
    }
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
    pub max_line_bytes: usize,
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            inactivity_timeout: Duration::from_mins(10),
            initial_backoff: Duration::from_secs(1),
            max_backoff: Duration::from_mins(1),
            rate_limit_backoff: Duration::from_mins(1),
            max_line_bytes: 64 * 1024,
        }
    }
}

#[cfg(test)]
mod tests;
