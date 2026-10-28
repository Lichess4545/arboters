use arboters::{app, config::Config, telemetry};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::from_env()?;
    telemetry::init(&config.telemetry);
    app::run(config).await
}
