use secrecy::SecretString;
use serde_json::json;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::*;
use crate::zulip::{BotEmail, InboxConfig};

const VISIBLE: u64 = 1234;

async fn reader() -> (MockServer, ZulipMessageReader) {
    let zulip = MockServer::start().await;
    let message = json!({
        "result": "success",
        "msg": "",
        "message": { "id": VISIBLE, "content": "**Good game!** :tada:", "sender_id": 11 },
    });
    Mock::given(method("GET"))
        .and(path(format!("/api/v1/messages/{VISIBLE}")))
        .and(query_param("apply_markdown", "false"))
        .respond_with(ResponseTemplate::new(200).set_body_json(message))
        .mount(&zulip)
        .await;
    let invalid = json!({ "result": "error", "msg": "Invalid message(s)", "code": "BAD_REQUEST" });
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(400).set_body_json(invalid))
        .mount(&zulip)
        .await;
    let config = Config {
        site: zulip.uri().parse().unwrap(),
        email: BotEmail::try_new("bot@chess.zulipchat.example.com").unwrap(),
        api_key: SecretString::from("key"),
        inbox: InboxConfig::default(),
    };
    (
        zulip,
        ZulipMessageReader::new(reqwest::Client::new(), &config),
    )
}

#[tokio::test]
async fn a_linked_message_is_read_as_its_raw_markdown() {
    let (_zulip, reader) = reader().await;

    let markdown = reader.read(MessageId(VISIBLE)).await.unwrap();

    assert_eq!(markdown.as_str(), "**Good game!** :tada:");
}

#[tokio::test]
async fn a_message_zulip_will_not_show_the_bot_is_unreadable() {
    let (_zulip, reader) = reader().await;

    let error = reader.read(MessageId(VISIBLE + 1)).await.unwrap_err();

    assert!(matches!(error, ReadError::Unreadable), "{error:?}");
}
