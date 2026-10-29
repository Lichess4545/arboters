use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use secrecy::SecretString;
use serde_json::{Value, json};
use tokio::net::TcpListener;

use super::*;
use crate::chat::{ChannelName, ChatUserId, TopicName};
use crate::game_monitor::GameEvent;
use crate::lichess::{self, Game, GameId, Player, Speed};
use crate::notifications::render;
use crate::zulip::BotEmail;

type CapturedForms = Arc<Mutex<Vec<BTreeMap<String, String>>>>;

struct FakeZulip {
    messenger: ZulipMessenger,
    forms: CapturedForms,
}

impl FakeZulip {
    async fn replying(reply: Value) -> Self {
        let forms = CapturedForms::default();
        let router = Router::new()
                .route(
                    "/api/v1/messages",
                    post(|State((forms, reply)): State<(CapturedForms, Value)>, body: String| async move {
                        let form = url::form_urlencoded::parse(body.as_bytes())
                            .into_owned()
                            .collect();
                        forms.lock().unwrap().push(form);
                        Json(reply)
                    }),
                )
                .with_state((forms.clone(), reply));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let settings = ZulipSettings {
            site: format!("http://{address}").parse().unwrap(),
            email: BotEmail::try_new("bot@chess.zulipchat.com").unwrap(),
            api_key: SecretString::from("key"),
        };
        Self {
            messenger: ZulipMessenger::new(&settings).unwrap(),
            forms,
        }
    }

    async fn accepting() -> Self {
        Self::replying(json!({ "result": "success", "msg": "", "id": 42 })).await
    }

    fn only_form(&self) -> BTreeMap<String, String> {
        let forms = self.forms.lock().unwrap();
        assert_eq!(forms.len(), 1);
        forms[0].clone()
    }
}

fn notification() -> Notification {
    let player = |name| Player {
        user_id: lichess::UserId::try_new(name).unwrap(),
        rating: None,
    };
    render(&GameEvent::Started(Game {
        id: GameId::new("abcd1234"),
        white: player("alice"),
        black: player("bob"),
        speed: Speed::Classical,
        rated: true,
    }))
}

fn form(fields: &[(&str, &str)]) -> BTreeMap<String, String> {
    fields
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect()
}

#[tokio::test]
async fn a_direct_message_is_sent_as_type_direct_to_a_bracketed_user_id_list() {
    let zulip = FakeZulip::accepting().await;
    let target = NotificationTarget::DirectMessage {
        user_id: ChatUserId(17),
    };

    zulip
        .messenger
        .send(&target, &notification())
        .await
        .unwrap();

    assert_eq!(
        zulip.only_form(),
        form(&[
            ("type", "direct"),
            ("to", "[17]"),
            ("content", notification().markdown()),
        ])
    );
}

#[tokio::test]
async fn a_channel_message_is_sent_as_type_channel_to_the_channel_and_topic() {
    let zulip = FakeZulip::accepting().await;
    let target = NotificationTarget::Channel {
        channel: ChannelName::try_new("4545").unwrap(),
        topic: TopicName::try_new("games").unwrap(),
    };

    zulip
        .messenger
        .send(&target, &notification())
        .await
        .unwrap();

    assert_eq!(
        zulip.only_form(),
        form(&[
            ("type", "channel"),
            ("to", "4545"),
            ("topic", "games"),
            ("content", notification().markdown()),
        ])
    );
}

#[tokio::test]
async fn a_non_success_result_from_zulip_is_a_delivery_error_carrying_its_error_code() {
    let zulip = FakeZulip::replying(json!({
        "result": "error",
        "msg": "Channel '4545' does not exist",
        "code": "STREAM_DOES_NOT_EXIST",
    }))
    .await;
    let target = NotificationTarget::DirectMessage {
        user_id: ChatUserId(17),
    };

    let error = zulip
        .messenger
        .send(&target, &notification())
        .await
        .unwrap_err();

    let rejection = error.cause.downcast_ref::<ZulipRejection>().unwrap();
    assert_eq!(rejection.code, Some(ErrorCode::StreamDoesNotExist));
}
