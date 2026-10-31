use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use serde::Deserialize;
use tokio::net::TcpListener;
use tokio::sync::watch;

use crate::health::{Availability, Health};

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HttpSettings {
    pub host: IpAddr,
    pub port: u16,
}

impl Default for HttpSettings {
    fn default() -> Self {
        Self {
            host: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: 8080,
        }
    }
}

impl HttpSettings {
    #[must_use]
    pub const fn address(&self) -> SocketAddr {
        SocketAddr::new(self.host, self.port)
    }
}

pub fn router(health: watch::Receiver<Option<Health>>) -> Router {
    Router::new()
        .route("/livez", get(liveness))
        .route("/readyz", get(readiness))
        .with_state(health)
}

pub async fn bind(settings: &HttpSettings) -> std::io::Result<TcpListener> {
    let listener = TcpListener::bind(settings.address()).await?;
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
