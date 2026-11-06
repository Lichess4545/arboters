use figment::Figment;
use figment::providers::Env;
use serde::Deserialize;

use super::{database, http, telemetry};
use crate::{game_monitor, health, heltour, league_games, lichess, responses, status, zulip};

pub const ENV_PREFIX: &str = "ARBOTERS_";
pub const ENV_NESTING_SEPARATOR: &str = "__";

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub http: http::Config,
    pub database: database::Config,
    #[serde(default)]
    pub health: health::Config,
    #[serde(default)]
    pub lichess: lichess::Config,
    pub heltour: heltour::Config,
    #[serde(default)]
    pub league_games: league_games::Config,
    pub zulip: zulip::Config,
    pub status: Option<status::Config>,
    #[serde(default)]
    pub responses: responses::Config,
    #[serde(default)]
    pub heltour_sync: heltour::sync::Config,
    #[serde(default)]
    pub game_recheck: game_monitor::recheck::Config,
    #[serde(default)]
    pub telemetry: telemetry::Config,
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
