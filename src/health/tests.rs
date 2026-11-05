use std::time::Duration;

use kameo::actor::Spawn;
use secrecy::SecretString;
use serde_json::json;
use url::Url;
use wiremock::matchers::{body_string, method, path};
use wiremock::{Mock, MockBuilder, MockServer, ResponseTemplate};

use super::*;
use crate::game_monitor::http_client;
use crate::heltour;
use crate::zulip::BotEmail;

const PATIENCE: Duration = Duration::from_secs(5);
const KNOWN_LICHESS_TOKEN: &str = "lip_known";

struct FakeServices(MockServer);

impl FakeServices {
    async fn start(heltour_answers: &[u16]) -> Self {
        let server = MockServer::start().await;
        for status in heltour_answers {
            heltour_token_test()
                .respond_with(ResponseTemplate::new(*status))
                .up_to_n_times(1)
                .mount(&server)
                .await;
        }
        if let Some(status) = heltour_answers.last() {
            heltour_token_test()
                .respond_with(ResponseTemplate::new(*status))
                .mount(&server)
                .await;
        }
        let owner = json!({ KNOWN_LICHESS_TOKEN: { "userId": "lichess4545" } });
        Mock::given(method("POST"))
            .and(path("/api/token/test"))
            .and(body_string(KNOWN_LICHESS_TOKEN))
            .respond_with(ResponseTemplate::new(200).set_body_json(owner))
            .mount(&server)
            .await;
        let bot = json!({ "result": "success", "msg": "", "user_id": 1, "full_name": "Arboters" });
        zulip_own_user()
            .respond_with(ResponseTemplate::new(200).set_body_json(bot))
            .mount(&server)
            .await;
        Self(server)
    }

    async fn heltour_answers(&self, status: u16) {
        heltour_token_test()
            .respond_with(ResponseTemplate::new(status))
            .with_priority(1)
            .mount(&self.0)
            .await;
    }

    async fn zulip_refuses_the_key(&self) {
        let refusal =
            json!({ "result": "error", "msg": "Invalid API key", "code": "INVALID_API_KEY" });
        zulip_own_user()
            .respond_with(ResponseTemplate::new(401).set_body_json(refusal))
            .with_priority(1)
            .mount(&self.0)
            .await;
    }

    fn heltour(&self) -> Heltour {
        let config = heltour::Config {
            base_url: Url::parse(&format!("{}/api/", self.0.uri())).unwrap(),
            token: SecretString::from("heltour_secret"),
        };
        Heltour::new(http_client().unwrap(), &config)
    }

    fn lichess(&self, token: &str) -> lichess::Api {
        let site = Url::parse(&self.0.uri()).unwrap();
        lichess::Api::new(
            http_client().unwrap(),
            &site,
            Some(SecretString::from(token)),
        )
    }

    fn zulip(&self) -> zulip::Api {
        let config = zulip::Config {
            site: Url::parse(&self.0.uri()).unwrap(),
            email: BotEmail::try_new("bot@chess.zulipchat.example.com").unwrap(),
            api_key: SecretString::from("zulip_secret"),
            inbox: zulip::InboxConfig::default(),
        };
        zulip::Api::new(http_client().unwrap(), &config)
    }
}

fn heltour_token_test() -> MockBuilder {
    Mock::given(method("GET")).and(path("/api/v2/token/test/"))
}

fn zulip_own_user() -> MockBuilder {
    Mock::given(method("GET")).and(path("/api/v1/users/me"))
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

fn checking(pool: PgPool, services: &FakeServices, lichess_token: Option<&str>) -> Checking {
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
        services.zulip(),
    );
    let health = health_check.health();
    Checking {
        health_check: HealthCheck::spawn(health_check),
        health,
    }
}

#[sqlx::test]
async fn when_our_tables_can_be_read_then_the_database_is_ready(pool: PgPool) {
    let services = FakeServices::start(&[204]).await;
    let mut checking = checking(pool, &services, None);
    assert_eq!(checking.next().await.postgres, Availability::Available(()));
}

#[sqlx::test]
async fn when_heltour_refuses_the_token_then_listeners_are_told_it_needs_a_person_and_then_that_it_is_available_once_it_accepts_it(
    pool: PgPool,
) {
    let services = FakeServices::start(&[401]).await;
    let mut checking = checking(pool, &services, None);
    assert_eq!(
        checking.next().await.heltour,
        Availability::Unavailable(Unavailable {
            recovery: Recovery::NeedsAPerson,
            error: "answered 401 Unauthorized".to_owned(),
        })
    );
    services.heltour_answers(204).await;
    assert_eq!(checking.next().await, everything_available());
}

#[sqlx::test]
async fn when_heltour_fails_once_and_passes_on_the_retry_then_listeners_are_never_told_it_is_unavailable(
    pool: PgPool,
) {
    let services = FakeServices::start(&[503, 204]).await;
    let mut checking = checking(pool, &services, None);
    assert_eq!(checking.next().await, everything_available());
}

#[sqlx::test(migrations = false)]
async fn when_our_tables_are_missing_then_health_stops_with_the_cause(pool: PgPool) {
    let services = FakeServices::start(&[204]).await;
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
    let services = FakeServices::start(&[204]).await;
    let mut checking = checking(pool, &services, Some(KNOWN_LICHESS_TOKEN));
    assert_eq!(
        checking.next().await.lichess,
        Some(Availability::Available(TokenOwner(
            lichess::UserId::try_new("lichess4545").unwrap()
        )))
    );
}

#[sqlx::test]
async fn when_zulip_refuses_the_api_key_then_listeners_are_told_it_needs_a_person(pool: PgPool) {
    let services = FakeServices::start(&[204]).await;
    services.zulip_refuses_the_key().await;
    let mut checking = checking(pool, &services, None);
    assert_eq!(
        checking.next().await.zulip,
        Availability::Unavailable(Unavailable {
            recovery: Recovery::NeedsAPerson,
            error: "zulip rejected the request (401 Unauthorized): Invalid API key".to_owned(),
        })
    );
}
