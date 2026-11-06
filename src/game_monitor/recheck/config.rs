use std::time::Duration;

use serde::Deserialize;

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    #[serde(with = "humantime_serde")]
    pub interval: Duration,
    #[serde(with = "humantime_serde")]
    pub rate_limit_backoff: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            interval: Duration::from_mins(15),
            // Lichess asks clients to wait a minute after a 429:
            // https://github.com/lichess-org/api/blob/master/doc/specs/lichess-api.yaml
            rate_limit_backoff: Duration::from_mins(1),
        }
    }
}
