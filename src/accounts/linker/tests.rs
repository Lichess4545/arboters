use kameo::actor::Spawn;
use secrecy::SecretString;
use serde_json::json;
use sqlx::PgPool;
use tokio::sync::mpsc;
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::*;
use crate::accounts::PostgresAccounts;
use crate::chat::Email;
use crate::game_monitor::http_client;
use crate::heltour;
use crate::testing::{Collector, linked_account};

const NEWCOMER: ChatUserId = ChatUserId(23);

async fn heltour_answering(answer: ResponseTemplate) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v2/players/by-email/"))
        .respond_with(answer)
        .mount(&server)
        .await;
    server
}

async fn join(pool: &PgPool, heltour: &MockServer, email: Option<&str>) {
    let config = heltour::Config {
        base_url: Url::parse(&format!("{}/api/", heltour.uri())).unwrap(),
        token: SecretString::from("heltour_secret"),
    };
    let linker = Linker::new(
        PostgresAccounts::new(pool.clone()),
        Heltour::new(http_client().unwrap(), &config),
    );
    let (watches, mut watching) = mpsc::unbounded_channel();
    let inbox = Collector::spawn(Collector(watches));
    let linker = Linker::spawn((linker, inbox.recipient::<WatchJoins>()));
    watching.recv().await.unwrap();
    linker
        .ask(Joined {
            user: NEWCOMER,
            email: email.map(|email| Email::try_new(email).unwrap()),
        })
        .await
        .unwrap();
}

#[sqlx::test]
async fn a_user_who_joins_with_a_players_email_is_linked_to_their_lichess_account(pool: PgPool) {
    let heltour = heltour_answering(
        ResponseTemplate::new(200).set_body_json(json!({ "name": "Alice", "id": "alice" })),
    )
    .await;

    join(&pool, &heltour, Some("alice@example.com")).await;

    assert_eq!(linked_account(&pool, NEWCOMER).await, ["alice"]);
}
