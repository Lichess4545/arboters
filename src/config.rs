use figment::Figment;
use figment::providers::Env;
use serde::Deserialize;

use crate::database::DatabaseSettings;
use crate::http::HttpSettings;
use crate::lichess::LichessSettings;
use crate::telemetry::TelemetrySettings;
use crate::watchlist::WatchlistSettings;
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
    pub zulip: Option<ZulipSettings>,
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
#[allow(clippy::result_large_err)]
mod tests {
    use std::time::Duration;

    use figment::Jail;
    use secrecy::ExposeSecret;

    use super::*;
    use crate::telemetry::LogFormat;

    const DATABASE_URL: &str = "postgres://localhost/arboters";

    fn in_clean_environment(test: impl FnOnce(&mut Jail) -> figment::Result<()>) {
        Jail::expect_with(|jail| {
            jail.clear_env();
            test(jail)
        });
    }

    fn load(jail: &mut Jail) -> figment::Result<Config> {
        jail.set_env("ARBOTERS_DATABASE__URL", DATABASE_URL);
        Config::from_env().map_err(|error| *error)
    }

    #[test]
    fn defaults_apply_when_only_the_database_is_configured() {
        in_clean_environment(|jail| {
            let config = load(jail)?;
            assert_eq!(config.http.port, 4545);
            assert_eq!(config.database.url.expose_secret(), DATABASE_URL);
            assert_eq!(config.database.max_connections, 5);
            assert_eq!(config.lichess.base_url.as_str(), "https://lichess.org/");
            assert_eq!(config.lichess.stream.max_users_per_connection, 900);
            assert!(config.zulip.is_none());
            assert_eq!(config.watchlist.refresh_interval, Duration::from_mins(5));
            assert_eq!(config.telemetry.log_format, LogFormat::Json);
            Ok(())
        });
    }

    #[test]
    fn nested_settings_are_read_from_namespaced_variables() {
        in_clean_environment(|jail| {
            jail.set_env("ARBOTERS_HTTP__PORT", "3000");
            jail.set_env("ARBOTERS_TELEMETRY__LOG_FORMAT", "pretty");
            jail.set_env("ARBOTERS_LICHESS__STREAM__INACTIVITY_TIMEOUT", "90s");
            jail.set_env("ARBOTERS_ZULIP__SITE", "https://chess.zulipchat.com");
            jail.set_env("ARBOTERS_ZULIP__EMAIL", "bot@chess.zulipchat.com");
            jail.set_env("ARBOTERS_ZULIP__API_KEY", "secret");
            let config = load(jail)?;
            assert_eq!(config.http.port, 3000);
            assert_eq!(config.telemetry.log_format, LogFormat::Pretty);
            assert_eq!(
                config.lichess.stream.inactivity_timeout,
                Duration::from_secs(90)
            );
            let zulip = config.zulip.expect("zulip is configured");
            assert_eq!(zulip.email.as_str(), "bot@chess.zulipchat.com");
            assert_eq!(zulip.api_key.expose_secret(), "secret");
            Ok(())
        });
    }

    #[test]
    fn unknown_variables_in_the_namespace_are_rejected() {
        in_clean_environment(|jail| {
            jail.set_env("ARBOTERS_HTTP__PROT", "1");
            assert!(load(jail).is_err());
            Ok(())
        });
    }

    #[test]
    fn malformed_bot_emails_are_rejected() {
        in_clean_environment(|jail| {
            jail.set_env("ARBOTERS_ZULIP__SITE", "https://chess.zulipchat.com");
            jail.set_env("ARBOTERS_ZULIP__EMAIL", "not-an-email");
            jail.set_env("ARBOTERS_ZULIP__API_KEY", "secret");
            assert!(load(jail).is_err());
            Ok(())
        });
    }
}
