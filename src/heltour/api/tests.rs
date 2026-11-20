use serde_json::{Value, json};
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::*;
use crate::game_monitor::http_client;
use crate::testing::user_id;

async fn heltour_answering(answer: ResponseTemplate) -> (MockServer, Heltour) {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v2/players/by-email/"))
        .and(body_json(json!({ "email": "alice@example.com" })))
        .respond_with(answer)
        .mount(&server)
        .await;
    let config = Config {
        base_url: Url::parse(&format!("{}/api/", server.uri())).unwrap(),
        token: SecretString::from("heltour_secret"),
    };
    (server, Heltour::new(http_client().unwrap(), &config))
}

async fn player_with_alices_email(
    status: u16,
    body: Value,
) -> Result<Option<lichess::UserId>, FindPlayerError> {
    let (_server, heltour) =
        heltour_answering(ResponseTemplate::new(status).set_body_json(body)).await;
    heltour
        .player_with_email(&Email::try_new("alice@example.com").unwrap())
        .await
}

#[tokio::test]
async fn the_player_with_the_email_is_found_by_their_lichess_id() {
    let found = player_with_alices_email(200, json!({ "name": "Alice", "id": "alice" })).await;

    assert_eq!(found.unwrap(), Some(user_id("alice")));
}

#[tokio::test]
async fn an_email_no_player_has_finds_no_one() {
    let found = player_with_alices_email(404, json!({ "detail": "Not found." })).await;

    assert_eq!(found.unwrap(), None);
}

#[tokio::test]
async fn an_email_more_than_one_player_has_is_ambiguous() {
    let found =
        player_with_alices_email(409, json!({ "detail": "More than one record matches." })).await;

    assert!(matches!(found, Err(FindPlayerError::Ambiguous)));
}

#[tokio::test]
async fn an_email_heltour_rejects_is_not_an_email() {
    let found =
        player_with_alices_email(400, json!({ "email": ["Enter a valid email address."] })).await;

    assert!(matches!(found, Err(FindPlayerError::NotAnEmail)));
}

#[tokio::test]
async fn a_token_without_the_permission_is_a_failed_request_not_an_unknown_player() {
    let found =
        player_with_alices_email(403, json!({ "detail": "You do not have permission." })).await;

    assert!(
        matches!(found, Err(FindPlayerError::Request(error)) if error.status() == Some(StatusCode::FORBIDDEN))
    );
}
