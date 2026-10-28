use serde::Deserialize;
use tracing_subscriber::EnvFilter;

const DEFAULT_FILTER: &str = "info,arboters=debug";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    #[default]
    Json,
    Pretty,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TelemetrySettings {
    pub log_format: LogFormat,
}

pub fn init(settings: &TelemetrySettings) {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_FILTER));
    let subscriber = tracing_subscriber::fmt().with_env_filter(filter);
    match settings.log_format {
        LogFormat::Json => subscriber.json().init(),
        LogFormat::Pretty => subscriber.pretty().init(),
    }
}
