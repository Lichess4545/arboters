use arboters::app::{self, config::Config, telemetry};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::from_env()?;
    let telemetry = telemetry::init(&config.telemetry)?;
    let outcome = app::run(config).await;
    tokio::task::spawn_blocking(move || telemetry.shutdown()).await?;
    outcome
}
