mod config;

use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use tokio::net::TcpListener;
use tokio::sync::watch;

use crate::health::{Availability, Health};
pub use config::Config;

pub fn router(health: watch::Receiver<Option<Health>>) -> Router {
    Router::new()
        .route("/livez", get(liveness))
        .route("/readyz", get(readiness))
        .with_state(health)
}

pub async fn bind(config: &Config) -> std::io::Result<TcpListener> {
    let listener = TcpListener::bind(config.address()).await?;
    tracing::info!(address = %listener.local_addr()?, "http server listening");
    Ok(listener)
}

pub async fn serve(
    listener: TcpListener,
    router: Router,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown)
        .await
}

async fn liveness() -> StatusCode {
    StatusCode::OK
}

async fn readiness(State(health): State<watch::Receiver<Option<Health>>>) -> StatusCode {
    if matches!(
        &*health.borrow(),
        Some(Health {
            postgres: Availability::Available(()),
            ..
        })
    ) {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    }
}

#[cfg(test)]
mod tests;
