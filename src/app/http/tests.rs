use axum::body::Body;
use axum::http::Request;
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;

use super::*;

const TIMEOUT: Duration = Duration::from_millis(300);

fn database_at(address: SocketAddr) -> PgPool {
    PgPoolOptions::new()
        .acquire_timeout(Duration::from_hours(1))
        .connect_lazy(&format!("postgres://nobody@{address}/nothing"))
        .unwrap()
}

async fn status(pool: PgPool, path: &str) -> StatusCode {
    status_with_token(pool, TokenHealth::Usable, path).await
}

async fn status_with_token(pool: PgPool, lichess_token: TokenHealth, path: &str) -> StatusCode {
    let request = Request::get(path).body(Body::empty()).unwrap();
    router(pool, TIMEOUT, watch::channel(lichess_token).1)
        .oneshot(request)
        .await
        .unwrap()
        .status()
}

#[tokio::test]
async fn livez_is_ok_while_the_database_is_unreachable() {
    let closed_port = SocketAddr::from(([127, 0, 0, 1], 1));
    assert_eq!(
        status(database_at(closed_port), "/livez").await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn livez_fails_once_the_lichess_token_is_unusable() {
    let closed_port = SocketAddr::from(([127, 0, 0, 1], 1));
    assert_eq!(
        status_with_token(database_at(closed_port), TokenHealth::Unusable, "/livez").await,
        StatusCode::SERVICE_UNAVAILABLE
    );
}

#[sqlx::test(migrations = false)]
async fn readyz_is_ok_when_the_database_answers(pool: PgPool) {
    assert_eq!(status(pool, "/readyz").await, StatusCode::OK);
}

#[tokio::test]
async fn readyz_fails_within_the_timeout_when_the_database_never_answers() {
    let silent = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let ready = tokio::time::timeout(
        TIMEOUT * 10,
        status(database_at(silent.local_addr().unwrap()), "/readyz"),
    )
    .await;
    assert_eq!(ready, Ok(StatusCode::SERVICE_UNAVAILABLE));
}
