use std::collections::BTreeMap;

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
use crate::zulip::{self, BotEmail, InboxConfig, ZulipMessenger};

const NEWCOMER: ChatUserId = ChatUserId(23);

type Form = BTreeMap<String, String>;

async fn heltour_answering(answer: ResponseTemplate) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v2/players/by-email/"))
        .respond_with(answer)
        .mount(&server)
        .await;
    server
}

async fn zulip_accepting_messages() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/messages"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "result": "success", "msg": "", "id": 42 })),
        )
        .mount(&server)
        .await;
    server
}

fn messenger_for(zulip: &MockServer) -> ZulipMessenger {
    let config = zulip::Config {
        site: zulip.uri().parse().unwrap(),
        email: BotEmail::try_new("bot@chess.zulipchat.example.com").unwrap(),
        api_key: SecretString::from("key"),
        inbox: InboxConfig::default(),
    };
    ZulipMessenger::new(http_client().unwrap(), &config)
}

fn heltour_at(server: &MockServer) -> Heltour {
    let config = heltour::Config {
        base_url: Url::parse(&format!("{}/api/", server.uri())).unwrap(),
        token: SecretString::from("heltour_secret"),
    };
    Heltour::new(http_client().unwrap(), &config)
}

async fn join(pool: &PgPool, heltour: &MockServer, email: Option<&str>) -> Vec<Form> {
    let zulip = zulip_accepting_messages().await;
    let linker = Linker::new(
        PostgresAccounts::new(pool.clone()),
        heltour_at(heltour),
        messenger_for(&zulip),
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
    sent_messages(&zulip).await
}

async fn sent_messages(zulip: &MockServer) -> Vec<Form> {
    zulip
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|request| {
            url::form_urlencoded::parse(&request.body)
                .into_owned()
                .collect()
        })
        .collect()
}

fn unidentified_welcome() -> Form {
    [
        ("type", "direct"),
        ("to", "[23]"),
        ("content", UNIDENTIFIED_WELCOME),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), value.to_owned()))
    .collect()
}

async fn heltour_refusing(status: u16) -> MockServer {
    heltour_answering(ResponseTemplate::new(status).set_body_json(json!({ "detail": "refused" })))
        .await
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

#[sqlx::test]
async fn a_user_who_joins_with_an_email_no_player_has_is_asked_to_reach_the_moderators(
    pool: PgPool,
) {
    let heltour = heltour_refusing(404).await;

    let sent = join(&pool, &heltour, Some("nobody@example.com")).await;

    assert_eq!(sent, [unidentified_welcome()]);
}

#[sqlx::test]
async fn a_user_who_joins_with_an_email_several_players_have_is_asked_to_reach_the_moderators(
    pool: PgPool,
) {
    let heltour = heltour_refusing(409).await;

    let sent = join(&pool, &heltour, Some("shared@example.com")).await;

    assert_eq!(sent, [unidentified_welcome()]);
}

#[sqlx::test]
async fn a_user_who_joins_without_a_real_email_is_asked_to_reach_the_moderators(pool: PgPool) {
    let heltour = heltour_refusing(404).await;

    let sent = join(&pool, &heltour, None).await;

    assert_eq!(sent, [unidentified_welcome()]);
}

#[sqlx::test]
async fn a_user_whose_email_heltour_rejects_is_sent_nothing(pool: PgPool) {
    let heltour = heltour_refusing(400).await;

    let sent = join(&pool, &heltour, Some("alice@example.com")).await;

    assert_eq!(sent, Vec::<Form>::new());
}

#[sqlx::test]
async fn a_user_who_joins_while_heltour_refuses_the_token_is_sent_nothing(pool: PgPool) {
    let heltour = heltour_refusing(403).await;

    let sent = join(&pool, &heltour, Some("alice@example.com")).await;

    assert_eq!(sent, Vec::<Form>::new());
}
