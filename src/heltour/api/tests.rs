use std::time::Duration;

use secrecy::SecretString;
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::*;
use crate::game_monitor::http_client;
use crate::testing::{tag, user_id};

const RETRIES: usize = 2;
const UPDATED: &str = r#"{"updated": 1}"#;
const RESULT_ALREADY_SET: &str = r#"{"updated": 0, "error": "result_already_set"}"#;

fn heltour(server: &MockServer) -> Heltour {
    let config = Config {
        base_url: Url::parse(&format!("{}/api/", server.uri())).unwrap(),
        token: SecretString::from("heltour_secret"),
        retry: RetryConfig {
            retries: RETRIES,
            initial_backoff: Duration::from_millis(1),
        },
    };
    Heltour::new(http_client().unwrap(), &config)
}

fn update() -> PairingUpdate {
    PairingUpdate {
        league: tag("classical"),
        white: user_id("alice"),
        black: user_id("bob"),
        game_link: Url::parse("https://lichess.org/abcd1234").unwrap(),
        result: None,
    }
}

fn update_pairing_answers(answer: ResponseTemplate) -> Mock {
    Mock::given(method("POST"))
        .and(path("/api/update_pairing/"))
        .respond_with(answer)
}

#[tokio::test]
async fn an_update_is_retried_after_a_server_error_or_rate_limit_until_heltour_takes_it() {
    let server = MockServer::start().await;
    for status in [503, 429] {
        update_pairing_answers(ResponseTemplate::new(status))
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;
    }
    update_pairing_answers(ResponseTemplate::new(200).set_body_string(UPDATED))
        .expect(1)
        .mount(&server)
        .await;
    heltour(&server).update_pairing(&update()).await.unwrap();
}

#[tokio::test]
async fn a_refusal_or_client_error_is_not_retried() {
    for answer in [
        ResponseTemplate::new(200).set_body_string(RESULT_ALREADY_SET),
        ResponseTemplate::new(404),
    ] {
        let server = MockServer::start().await;
        update_pairing_answers(answer)
            .expect(1)
            .mount(&server)
            .await;
        heltour(&server)
            .update_pairing(&update())
            .await
            .unwrap_err();
    }
}

#[tokio::test]
async fn an_update_gives_up_after_the_configured_retries() {
    let server = MockServer::start().await;
    update_pairing_answers(ResponseTemplate::new(503))
        .expect(1 + RETRIES as u64)
        .mount(&server)
        .await;
    heltour(&server)
        .update_pairing(&update())
        .await
        .unwrap_err();
}
