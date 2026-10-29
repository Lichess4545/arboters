use std::time::Duration;

use figment::Jail;

use super::*;
use crate::app::telemetry::LogFormat;

const KUBELET_PROBE_TIMEOUT: Duration = Duration::from_secs(1);

#[expect(clippy::result_large_err)]
fn in_clean_environment(test: impl FnOnce(&mut Jail)) {
    Jail::expect_with(|jail| {
        jail.clear_env();
        test(jail);
        Ok(())
    });
}

fn load(jail: &mut Jail) -> Result<Config, Box<figment::Error>> {
    jail.set_env("ARBOTERS_DATABASE__URL", "postgres://localhost/arboters");
    jail.set_env("ARBOTERS_HELTOUR__BASE_URL", "https://heltour.example/api/");
    Config::from_env()
}

#[test]
fn with_only_the_required_variables_set_every_other_setting_takes_its_default() {
    in_clean_environment(|jail| {
        let config = load(jail).unwrap();
        assert!(config.http.host.is_loopback());
        assert_eq!(config.http.port, 8080);
        assert!(config.http.readiness_timeout < KUBELET_PROBE_TIMEOUT);
        assert_eq!(config.lichess.base_url.as_str(), "https://lichess.org/");
        assert_eq!(config.lichess.token_check_interval, Duration::from_mins(15));
        assert_eq!(config.watchlist.refresh_interval, Duration::from_mins(5));
        assert_eq!(config.telemetry.log_format, LogFormat::Json);
    });
}

#[test]
fn heltour_has_no_default_location() {
    in_clean_environment(|jail| {
        jail.set_env("ARBOTERS_DATABASE__URL", "postgres://localhost/arboters");
        assert!(Config::from_env().is_err());
    });
}

#[test]
fn the_variables_deployments_set_are_recognized() {
    in_clean_environment(|jail| {
        jail.set_env("ARBOTERS_HTTP__HOST", "0.0.0.0");
        jail.set_env("ARBOTERS_HTTP__PORT", "8080");
        jail.set_env("ARBOTERS_TELEMETRY__LOG_FORMAT", "pretty");
        jail.set_env("ARBOTERS_ZULIP__SITE", "https://chess.zulipchat.com");
        jail.set_env("ARBOTERS_ZULIP__EMAIL", "arboters@chess.zulipchat.com");
        jail.set_env("ARBOTERS_ZULIP__API_KEY", "secret");
        jail.set_env("ARBOTERS_LICHESS__TOKEN", "lip_secret");
        jail.set_env("ARBOTERS_HELTOUR__TOKEN", "secret");
        jail.set_env("ARBOTERS_STATUS__CHANNEL", "arboters-status");
        load(jail).unwrap();
    });
}

#[test]
fn a_misspelled_variable_in_any_section_is_rejected() {
    for misspelled in [
        "ARBOTERS_HTTPS__PORT",
        "ARBOTERS_HTTP__PROT",
        "ARBOTERS_DATABASE__URI",
        "ARBOTERS_LICHESS__TOKN",
        "ARBOTERS_LICHESS__STREAM__MAX_BACKOF",
        "ARBOTERS_HELTOUR__TOKN",
        "ARBOTERS_WATCHLIST__REFRESH",
        "ARBOTERS_TELEMETRY__FORMAT",
        "ARBOTERS_STATUS__CHANEL",
    ] {
        in_clean_environment(|jail| {
            jail.set_env(misspelled, "1");
            assert!(load(jail).is_err(), "{misspelled} was accepted");
        });
    }
}

#[test]
fn the_status_topic_cannot_be_configured_because_each_instance_names_its_own() {
    in_clean_environment(|jail| {
        jail.set_env("ARBOTERS_STATUS__CHANNEL", "arboters-status");
        jail.set_env("ARBOTERS_STATUS__TOPIC", "arboters");
        assert!(load(jail).is_err(), "ARBOTERS_STATUS__TOPIC was accepted");
    });
}

#[test]
fn a_response_waits_five_seconds_before_it_fires_again_unless_configured_otherwise() {
    in_clean_environment(|jail| {
        assert_eq!(
            load(jail).unwrap().responses.cooldown,
            Duration::from_secs(5)
        );
        jail.set_env("ARBOTERS_RESPONSES__COOLDOWN", "2m");
        assert_eq!(
            load(jail).unwrap().responses.cooldown,
            Duration::from_mins(2)
        );
    });
}
