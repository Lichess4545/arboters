use opentelemetry::trace::TracerProvider as _;
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_otlp::{ExporterBuildError, LogExporter, SpanExporter};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::logs::SdkLoggerProvider;
use opentelemetry_sdk::trace::SdkTracerProvider;
use serde::Deserialize;
use tracing_subscriber::filter::{LevelFilter, Targets};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::{SubscriberInitExt, TryInitError};
use tracing_subscriber::{EnvFilter, Layer, fmt};

const SERVICE_NAME: &str = env!("CARGO_PKG_NAME");
const DEFAULT_FILTER: &str = "info,arboters=debug";
const OTLP_ENDPOINT_VARIABLE: &str = "OTEL_EXPORTER_OTLP_ENDPOINT";
const EXPORTER_INTERNALS: [&str; 6] = [
    "hyper",
    "h2",
    "reqwest",
    "tower",
    "opentelemetry",
    "opentelemetry_sdk",
];

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

#[derive(Debug, thiserror::Error)]
pub enum TelemetryError {
    #[error("could not build the OTLP exporter")]
    Exporter(#[from] ExporterBuildError),
    #[error("a global tracing subscriber is already installed")]
    AlreadyInitialized(#[from] TryInitError),
}

#[derive(Debug)]
pub struct Telemetry {
    otlp: Option<OtlpPipeline>,
}

#[derive(Debug)]
struct OtlpPipeline {
    traces: SdkTracerProvider,
    logs: SdkLoggerProvider,
}

impl OtlpPipeline {
    fn from_environment() -> Result<Option<Self>, ExporterBuildError> {
        if std::env::var_os(OTLP_ENDPOINT_VARIABLE).is_none() {
            return Ok(None);
        }
        let resource = Resource::builder().with_service_name(SERVICE_NAME).build();
        let traces = SdkTracerProvider::builder()
            .with_batch_exporter(SpanExporter::builder().with_http().build()?)
            .with_resource(resource.clone())
            .build();
        let logs = SdkLoggerProvider::builder()
            .with_batch_exporter(LogExporter::builder().with_http().build()?)
            .with_resource(resource)
            .build();
        Ok(Some(Self { traces, logs }))
    }

    fn shutdown(self) {
        if let Err(error) = self.traces.shutdown() {
            tracing::warn!(error = %error, "trace exporter did not shut down cleanly");
        }
        if let Err(error) = self.logs.shutdown() {
            tracing::warn!(error = %error, "log exporter did not shut down cleanly");
        }
    }
}

pub fn init(settings: &TelemetrySettings) -> Result<Telemetry, TelemetryError> {
    let otlp = OtlpPipeline::from_environment()?;
    let console = match settings.log_format {
        LogFormat::Json => fmt::layer().json().boxed(),
        LogFormat::Pretty => fmt::layer().pretty().boxed(),
    };
    let span_export = otlp.as_ref().map(|pipeline| {
        tracing_opentelemetry::layer().with_tracer(pipeline.traces.tracer(SERVICE_NAME))
    });
    let log_export = otlp.as_ref().map(|pipeline| {
        OpenTelemetryTracingBridge::new(&pipeline.logs)
            .with_filter(filter_without_exporter_internals())
    });
    tracing_subscriber::registry()
        .with(requested_filter())
        .with(console)
        .with(span_export)
        .with(log_export)
        .try_init()?;
    if otlp.is_some() {
        tracing::info!(
            service = SERVICE_NAME,
            "exporting traces and logs over OTLP"
        );
    }
    Ok(Telemetry { otlp })
}

impl Telemetry {
    pub fn shutdown(self) {
        if let Some(pipeline) = self.otlp {
            pipeline.shutdown();
        }
    }
}

fn requested_filter() -> EnvFilter {
    EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_FILTER))
}

fn filter_without_exporter_internals() -> Targets {
    Targets::new()
        .with_default(LevelFilter::TRACE)
        .with_targets(EXPORTER_INTERNALS.map(|target| (target, LevelFilter::OFF)))
}
