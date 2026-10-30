use std::time::Duration;

use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ResponsesSettings {
    #[serde(with = "humantime_serde")]
    pub cooldown: Duration,
}

impl Default for ResponsesSettings {
    fn default() -> Self {
        Self {
            cooldown: Duration::from_secs(5),
        }
    }
}
