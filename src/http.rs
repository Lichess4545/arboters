use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use serde::Deserialize;
use sqlx::{Connection, PgPool};
use tokio::net::TcpListener;

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
            port: 4545,
        }
    }
}

impl HttpSettings {
    #[must_use]
    pub const fn address(&self) -> SocketAddr {
        SocketAddr::new(self.host, self.port)
    }
}

pub fn router(pool: PgPool) -> Router {
    Router::new().route("/health", get(health)).with_state(pool)
}

pub async fn serve(
    settings: &HttpSettings,
    router: Router,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    let listener = TcpListener::bind(settings.address()).await?;
    tracing::info!(address = %listener.local_addr()?, "http server listening");
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown)
        .await
}

async fn health(State(pool): State<PgPool>) -> StatusCode {
    let database_reachable = match pool.acquire().await {
        Ok(mut connection) => connection.ping().await.is_ok(),
        Err(_) => false,
    };
    if database_reachable {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use axum::body::Body;
    use axum::http::Request;
    use sqlx::postgres::PgPoolOptions;
    use tower::ServiceExt;

    use super::*;

    async fn health_status(pool: PgPool) -> StatusCode {
        let request = Request::get("/health").body(Body::empty()).unwrap();
        router(pool).oneshot(request).await.unwrap().status()
    }

    #[sqlx::test(migrations = false)]
    async fn health_is_ok_when_the_database_answers(pool: PgPool) {
        assert_eq!(health_status(pool).await, StatusCode::OK);
    }

    #[tokio::test]
    async fn health_is_unavailable_when_the_database_is_unreachable() {
        let unreachable = PgPoolOptions::new()
            .acquire_timeout(Duration::from_millis(200))
            .connect_lazy("postgres://nobody@127.0.0.1:1/nothing")
            .unwrap();
        assert_eq!(
            health_status(unreachable).await,
            StatusCode::SERVICE_UNAVAILABLE
        );
    }
}
