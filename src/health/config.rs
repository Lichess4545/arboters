use std::time::Duration;

use serde::Deserialize;

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    #[serde(with = "humantime_serde")]
    pub interval: Duration,
    #[serde(with = "humantime_serde")]
    pub timeout: Duration,
    pub retries: usize,
    #[serde(with = "humantime_serde")]
    pub initial_backoff: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(30),
            timeout: Duration::from_secs(5),
            retries: 2,
            initial_backoff: Duration::from_secs(1),
        }
    }
}
