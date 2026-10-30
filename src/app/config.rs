use figment::Figment;
use figment::providers::Env;
use serde::Deserialize;

use super::database::DatabaseSettings;
use super::http::HttpSettings;
use super::telemetry::TelemetrySettings;
use crate::game_monitor::LichessSettings;
use crate::game_monitor::watchlist::{HeltourSettings, WatchlistSettings};
use crate::status::StatusSettings;
use crate::zulip::ZulipSettings;

pub const ENV_PREFIX: &str = "ARBOTERS_";
pub const ENV_NESTING_SEPARATOR: &str = "__";

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub http: HttpSettings,
    pub database: DatabaseSettings,
    #[serde(default)]
    pub lichess: LichessSettings,
    pub heltour: HeltourSettings,
    pub zulip: Option<ZulipSettings>,
    pub status: Option<StatusSettings>,
    #[serde(default)]
    pub watchlist: WatchlistSettings,
    #[serde(default)]
    pub telemetry: TelemetrySettings,
}

impl Config {
    #[must_use]
    pub fn figment() -> Figment {
        Figment::from(Env::prefixed(ENV_PREFIX).split(ENV_NESTING_SEPARATOR))
    }

    pub fn from_env() -> Result<Self, Box<figment::Error>> {
        Self::figment().extract().map_err(Box::new)
    }
}

#[cfg(test)]
mod tests;
