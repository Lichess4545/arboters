use tokio::signal::unix::{SignalKind, signal};

use crate::config::Config;
use crate::{database, http};

pub async fn run(config: Config) -> anyhow::Result<()> {
    let pool = database::connect(&config.database).await?;
    http::serve(&config.http, http::router(pool), shutdown_requested()).await?;
    Ok(())
}

async fn shutdown_requested() {
    let terminate = async {
        match signal(SignalKind::terminate()) {
            Ok(mut terminations) => {
                terminations.recv().await;
            }
            Err(error) => {
                tracing::warn!(error = %error, "cannot listen for SIGTERM");
                std::future::pending::<()>().await;
            }
        }
    };
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        () = terminate => {}
    }
    tracing::info!("shutdown requested");
}
