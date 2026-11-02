use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::routing::{get, post};
use axum::{Json, Router};
use kameo::actor::Spawn;
use reqwest::StatusCode;
use secrecy::SecretString;
use serde_json::{Value, json};
use tokio::net::TcpListener;
use url::Url;

use super::*;
use crate::game_monitor::http_client;
use crate::heltour::HeltourSettings;

const PATIENCE: Duration = Duration::from_secs(5);
const KNOWN_LICHESS_TOKEN: &str = "lip_known";

struct FakeServices {
    heltour_answers: Arc<Mutex<VecDeque<StatusCode>>>,
    address: std::net::SocketAddr,
}

impl FakeServices {
    async fn start(heltour_answers: &[StatusCode]) -> Self {
        let heltour_answers = Arc::new(Mutex::new(heltour_answers.iter().copied().collect()));
        let answering = Arc::clone(&heltour_answers);
        let router = Router::new()
            .route(
                "/api/v2/token/test/",
                get(move || std::future::ready(next_answer(&answering))),
            )
            .route("/api/token/test", post(lichess_token_test));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        Self {
            heltour_answers,
            address,
        }
    }

    fn heltour_answers(&self, status: StatusCode) {
        *self.heltour_answers.lock().unwrap() = VecDeque::from([status]);
    }

    fn heltour(&self) -> Heltour {
        let settings = HeltourSettings {
            base_url: Url::parse(&format!("http://{}/api/", self.address)).unwrap(),
            token: SecretString::from("heltour_secret"),
        };
        Heltour::new(http_client().unwrap(), &settings).unwrap()
    }

    fn lichess(&self, token: Option<&str>) -> LichessSettings {
        LichessSettings {
            base_url: Url::parse(&format!("http://{}/", self.address)).unwrap(),
            token: token.map(SecretString::from),
            ..LichessSettings::default()
        }
    }
}

fn next_answer(answers: &Mutex<VecDeque<StatusCode>>) -> StatusCode {
    let mut answers = answers.lock().unwrap();
    if answers.len() > 1 {
        answers.pop_front().unwrap()
    } else {
        answers[0]
    }
}

async fn lichess_token_test(token: String) -> Json<Value> {
    let owner = (token == KNOWN_LICHESS_TOKEN).then(|| json!({ "userId": "lichess4545" }));
    Json(json!({ token: owner }))
}

fn everything_available() -> Health {
    Health {
        postgres: Availability::Available(()),
        heltour: Availability::Available(()),
        lichess: None,
        zulip: None,
    }
}

struct Checking {
    health_check: ActorRef<HealthCheck>,
    health: watch::Receiver<Option<Health>>,
}

impl Checking {
    async fn next(&mut self) -> Health {
        tokio::time::timeout(PATIENCE, self.health.changed())
            .await
            .unwrap()
            .unwrap();
        self.health.borrow_and_update().clone().unwrap()
    }
}

fn checking(pool: PgPool, services: &FakeServices, lichess_token: Option<&str>) -> Checking {
    let config = Config {
        interval: Duration::from_millis(10),
        timeout: Duration::from_secs(1),
        retries: 2,
        initial_backoff: Duration::from_millis(10),
    };
    let health_check = HealthCheck::new(
        config,
        http_client().unwrap(),
        pool,
        services.heltour(),
        &services.lichess(lichess_token),
        None,
    )
    .unwrap();
    let health = health_check.health();
    Checking {
        health_check: HealthCheck::spawn(health_check),
        health,
    }
}

#[sqlx::test]
async fn when_our_tables_can_be_read_then_the_database_is_ready(pool: PgPool) {
    let services = FakeServices::start(&[StatusCode::NO_CONTENT]).await;
    let mut checking = checking(pool, &services, None);
    assert_eq!(checking.next().await.postgres, Availability::Available(()));
}

#[sqlx::test]
async fn when_heltour_refuses_the_token_then_listeners_are_told_it_needs_a_person_and_then_that_it_is_available_once_it_accepts_it(
    pool: PgPool,
) {
    let services = FakeServices::start(&[StatusCode::UNAUTHORIZED]).await;
    let mut checking = checking(pool, &services, None);
    assert_eq!(
        checking.next().await.heltour,
        Availability::Unavailable(Unavailable {
            recovery: Recovery::NeedsAPerson,
            error: "answered 401 Unauthorized".to_owned(),
        })
    );
    services.heltour_answers(StatusCode::NO_CONTENT);
    assert_eq!(checking.next().await, everything_available());
}

#[sqlx::test]
async fn when_heltour_fails_once_and_passes_on_the_retry_then_listeners_are_never_told_it_is_unavailable(
    pool: PgPool,
) {
    let services =
        FakeServices::start(&[StatusCode::SERVICE_UNAVAILABLE, StatusCode::NO_CONTENT]).await;
    let mut checking = checking(pool, &services, None);
    assert_eq!(checking.next().await, everything_available());
}

#[sqlx::test(migrations = false)]
async fn when_our_tables_are_missing_then_health_stops_with_the_cause(pool: PgPool) {
    let services = FakeServices::start(&[StatusCode::NO_CONTENT]).await;
    let checking = checking(pool, &services, None);
    let stopped = tokio::time::timeout(PATIENCE, HealthCheck::stopped(&checking.health_check))
        .await
        .unwrap();
    assert!(
        stopped
            .to_string()
            .starts_with("Postgres failed its health check"),
        "{stopped}"
    );
}

#[sqlx::test]
async fn when_lichess_knows_the_token_then_the_snapshot_carries_its_owner(pool: PgPool) {
    let services = FakeServices::start(&[StatusCode::NO_CONTENT]).await;
    let mut checking = checking(pool, &services, Some(KNOWN_LICHESS_TOKEN));
    assert_eq!(
        checking.next().await.lichess,
        Some(Availability::Available(TokenOwner(
            lichess::UserId::try_new("lichess4545").unwrap()
        )))
    );
}
