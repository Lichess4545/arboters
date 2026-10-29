use std::collections::BTreeMap;

use axum::extract::{Path, Query};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use secrecy::SecretString;
use serde_json::{Value, json};
use tokio::net::TcpListener;

use super::*;
use crate::zulip::{BotEmail, InboxSettings};

const VISIBLE: u64 = 1234;

async fn message(
    Path(id): Path<u64>,
    Query(query): Query<BTreeMap<String, String>>,
) -> (StatusCode, Json<Value>) {
    if id != VISIBLE {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "result": "error", "msg": "Invalid message(s)", "code": "BAD_REQUEST" })),
        );
    }
    let content = if query.get("apply_markdown").map(String::as_str) == Some("false") {
        "**Good game!** :tada:"
    } else {
        "<p><strong>Good game!</strong></p>"
    };
    (
        StatusCode::OK,
        Json(json!({
            "result": "success",
            "msg": "",
            "message": { "id": id, "content": content, "sender_id": 11 },
        })),
    )
}

async fn reader() -> ZulipMessageReader {
    let router = Router::new().route("/api/v1/messages/{id}", get(message));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let settings = ZulipSettings {
        site: format!("http://{address}").parse().unwrap(),
        email: BotEmail::try_new("bot@chess.zulipchat.com").unwrap(),
        api_key: SecretString::from("key"),
        inbox: InboxSettings::default(),
    };
    ZulipMessageReader::new(reqwest::Client::new(), &settings)
}

#[tokio::test]
async fn a_linked_message_is_read_as_its_raw_markdown() {
    let reader = reader().await;

    let markdown = reader.read(MessageId(VISIBLE)).await.unwrap();

    assert_eq!(markdown.as_str(), "**Good game!** :tada:");
}

#[tokio::test]
async fn a_message_zulip_will_not_show_the_bot_is_unreadable() {
    let reader = reader().await;

    let error = reader.read(MessageId(VISIBLE + 1)).await.unwrap_err();

    assert!(matches!(error, ReadError::Unreadable), "{error:?}");
}
