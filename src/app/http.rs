use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use serde::Deserialize;
use sqlx::{Connection, PgPool};
use tokio::net::TcpListener;
use tokio::sync::watch;

use crate::game_monitor::TokenState;

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HttpSettings {
    pub host: IpAddr,
    pub port: u16,
    #[serde(with = "humantime_serde")]
    pub readiness_timeout: Duration,
}

impl Default for HttpSettings {
    fn default() -> Self {
        Self {
            host: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: 8080,
            readiness_timeout: Duration::from_millis(800),
        }
    }
}

impl HttpSettings {
    #[must_use]
    pub const fn address(&self) -> SocketAddr {
        SocketAddr::new(self.host, self.port)
    }
}

#[derive(Clone, Debug)]
struct Probes {
    pool: PgPool,
    readiness_timeout: Duration,
    lichess_token: watch::Receiver<TokenState>,
}

pub fn router(
    pool: PgPool,
    readiness_timeout: Duration,
    lichess_token: watch::Receiver<TokenState>,
) -> Router {
    Router::new()
        .route("/livez", get(liveness))
        .route("/readyz", get(readiness))
        .with_state(Probes {
            pool,
            readiness_timeout,
            lichess_token,
        })
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

async fn liveness(State(probes): State<Probes>) -> StatusCode {
    if *probes.lichess_token.borrow() == TokenState::Invalid {
        tracing::warn!("liveness probe failed: lichess reports the token invalid");
        StatusCode::SERVICE_UNAVAILABLE
    } else {
        StatusCode::OK
    }
}

async fn readiness(State(probes): State<Probes>) -> StatusCode {
    let ping = async {
        let mut connection = probes.pool.acquire().await?;
        connection.ping().await
    };
    match tokio::time::timeout(probes.readiness_timeout, ping).await {
        Ok(Ok(())) => StatusCode::OK,
        Ok(Err(error)) => {
            tracing::warn!(error = %error, "readiness probe failed: database unavailable");
            StatusCode::SERVICE_UNAVAILABLE
        }
        Err(_) => {
            tracing::warn!(timeout = ?probes.readiness_timeout, "readiness probe failed: database did not answer");
            StatusCode::SERVICE_UNAVAILABLE
        }
    }
}

#[cfg(test)]
mod tests;
