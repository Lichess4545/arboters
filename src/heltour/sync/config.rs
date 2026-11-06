use std::time::Duration;

use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    #[serde(with = "humantime_serde")]
    pub refresh_interval: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            refresh_interval: Duration::from_mins(5),
        }
    }
}
