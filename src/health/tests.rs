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
use crate::heltour;
use crate::zulip::BotEmail;

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
            .route("/api/token/test", post(lichess_token_test))
            .route("/api/v1/users/me", get(zulip_bot))
            .route("/refusing/api/v1/users/me", get(zulip_refusing_the_key));
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
        let config = heltour::Config {
            base_url: Url::parse(&format!("http://{}/api/", self.address)).unwrap(),
            token: SecretString::from("heltour_secret"),
        };
        Heltour::new(http_client().unwrap(), &config)
    }

    fn lichess(&self, token: &str) -> lichess::Api {
        let site = Url::parse(&format!("http://{}/", self.address)).unwrap();
        lichess::Api::new(http_client().unwrap(), &site, SecretString::from(token))
    }

    fn zulip(&self) -> zulip::Api {
        self.zulip_at("")
    }

    fn refusing_zulip(&self) -> zulip::Api {
        self.zulip_at("refusing/")
    }

    fn zulip_at(&self, path: &str) -> zulip::Api {
        let config = zulip::Config {
            site: Url::parse(&format!("http://{}/{path}", self.address)).unwrap(),
            email: BotEmail::try_new("bot@example.com").unwrap(),
            api_key: SecretString::from("zulip_key"),
            inbox: zulip::InboxConfig::default(),
        };
        zulip::Api::new(http_client().unwrap(), &config)
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

async fn zulip_bot() -> Json<Value> {
    Json(json!({ "result": "success", "msg": "", "user_id": 1, "full_name": "Arboters" }))
}

async fn zulip_refusing_the_key() -> (StatusCode, Json<Value>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({ "result": "error", "msg": "Invalid API key", "code": "INVALID_API_KEY" })),
    )
}

fn everything_available() -> Health {
    Health {
        postgres: Availability::Available(()),
        heltour: Availability::Available(()),
        lichess: None,
        zulip: Availability::Available(()),
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

fn checking(
    pool: PgPool,
    services: &FakeServices,
    lichess_token: Option<&str>,
    zulip: zulip::Api,
) -> Checking {
    let config = Config {
        interval: Duration::from_millis(10),
        timeout: Duration::from_secs(1),
        retries: 2,
        initial_backoff: Duration::from_millis(10),
    };
    let health_check = HealthCheck::new(
        config,
        pool,
        services.heltour(),
        lichess_token.map(|token| services.lichess(token)),
        zulip,
    );
    let health = health_check.health();
    Checking {
        health_check: HealthCheck::spawn(health_check),
        health,
    }
}

#[sqlx::test]
async fn when_our_tables_can_be_read_then_the_database_is_ready(pool: PgPool) {
    let services = FakeServices::start(&[StatusCode::NO_CONTENT]).await;
    let mut checking = checking(pool, &services, None, services.zulip());
    assert_eq!(checking.next().await.postgres, Availability::Available(()));
}

#[sqlx::test]
async fn when_heltour_refuses_the_token_then_listeners_are_told_it_needs_a_person_and_then_that_it_is_available_once_it_accepts_it(
    pool: PgPool,
) {
    let services = FakeServices::start(&[StatusCode::UNAUTHORIZED]).await;
    let mut checking = checking(pool, &services, None, services.zulip());
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
    let mut checking = checking(pool, &services, None, services.zulip());
    assert_eq!(checking.next().await, everything_available());
}

#[sqlx::test(migrations = false)]
async fn when_our_tables_are_missing_then_health_stops_with_the_cause(pool: PgPool) {
    let services = FakeServices::start(&[StatusCode::NO_CONTENT]).await;
    let checking = checking(pool, &services, None, services.zulip());
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
    let mut checking = checking(pool, &services, Some(KNOWN_LICHESS_TOKEN), services.zulip());
    assert_eq!(
        checking.next().await.lichess,
        Some(Availability::Available(TokenOwner(
            lichess::UserId::try_new("lichess4545").unwrap()
        )))
    );
}

#[sqlx::test]
async fn when_zulip_refuses_the_api_key_then_listeners_are_told_it_needs_a_person(pool: PgPool) {
    let services = FakeServices::start(&[StatusCode::NO_CONTENT]).await;
    let mut checking = checking(pool, &services, None, services.refusing_zulip());
    assert_eq!(
        checking.next().await.zulip,
        Availability::Unavailable(Unavailable {
            recovery: Recovery::NeedsAPerson,
            error: "zulip rejected the request (401 Unauthorized): Invalid API key".to_owned(),
        })
    );
}
