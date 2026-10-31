use axum::body::Body;
use axum::http::Request;
use tower::ServiceExt;

use super::*;
use crate::health::{Recovery, Unavailable};

fn postgres(postgres: Availability) -> Health {
    Health {
        postgres,
        heltour: Availability::Available(()),
        lichess: None,
        zulip: None,
    }
}

fn postgres_unavailable() -> Health {
    postgres(Availability::Unavailable(Unavailable {
        recovery: Recovery::Recoverable,
        error: "connection refused".to_owned(),
    }))
}

async fn status(health: Option<Health>, path: &str) -> StatusCode {
    let request = Request::get(path).body(Body::empty()).unwrap();
    router(watch::channel(health).1)
        .oneshot(request)
        .await
        .unwrap()
        .status()
}

#[tokio::test]
async fn livez_answers_200_while_the_database_is_not_ready() {
    assert_eq!(
        status(Some(postgres_unavailable()), "/livez").await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn readyz_answers_200_once_health_finds_the_database_ready() {
    assert_eq!(
        status(Some(postgres(Availability::Available(()))), "/readyz").await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn readyz_answers_503_before_health_has_checked() {
    assert_eq!(
        status(None, "/readyz").await,
        StatusCode::SERVICE_UNAVAILABLE
    );
}

#[tokio::test]
async fn readyz_answers_503_while_health_does_not_find_the_database_ready() {
    assert_eq!(
        status(Some(postgres_unavailable()), "/readyz").await,
        StatusCode::SERVICE_UNAVAILABLE
    );
}
