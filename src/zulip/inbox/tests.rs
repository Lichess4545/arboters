use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use kameo::actor::Spawn;
use secrecy::SecretString;
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::sync::{Mutex, mpsc};

use super::*;
use crate::chat::{ChannelName, Role, TopicName};
use crate::zulip::BotEmail;

const BOT: u64 = 99;
const MEMBER: u64 = 10;
const MODERATOR: u64 = 11;

type Form = BTreeMap<String, String>;
type Reply = (StatusCode, Value);

#[derive(Clone)]
struct FakeState {
    replies: Arc<Mutex<mpsc::UnboundedReceiver<Reply>>>,
    registrations: mpsc::UnboundedSender<Form>,
    registered: Arc<std::sync::Mutex<u32>>,
    polls: mpsc::UnboundedSender<(String, String)>,
    sent: mpsc::UnboundedSender<Form>,
}

fn form(body: &str) -> Form {
    url::form_urlencoded::parse(body.as_bytes())
        .into_owned()
        .collect()
}

fn success(fields: Value) -> Value {
    let mut reply = json!({ "result": "success", "msg": "" });
    if let (Value::Object(reply), Value::Object(fields)) = (&mut reply, fields) {
        reply.extend(fields);
    }
    reply
}

async fn own_user() -> Json<Value> {
    Json(success(json!({
        "user_id": BOT,
        "full_name": "Arboters",
        "email": "arboters-bot@chess.zulipchat.com",
    })))
}

async fn user(Path(user_id): Path<u64>) -> Json<Value> {
    let role = if user_id == MODERATOR { 300 } else { 400 };
    Json(success(
        json!({ "user": { "user_id": user_id, "role": role } }),
    ))
}

async fn register(State(state): State<FakeState>, body: String) -> Json<Value> {
    let number = {
        let mut registered = state.registered.lock().unwrap();
        *registered += 1;
        *registered
    };
    state.registrations.send(form(&body)).unwrap();
    Json(success(json!({
        "queue_id": format!("queue-{number}"),
        "last_event_id": -1,
    })))
}

async fn events(
    State(state): State<FakeState>,
    Query(query): Query<BTreeMap<String, String>>,
) -> (StatusCode, Json<Value>) {
    state
        .polls
        .send((query["queue_id"].clone(), query["last_event_id"].clone()))
        .unwrap();
    let reply = state.replies.lock().await.recv().await;
    match reply {
        Some((status, body)) => (status, Json(body)),
        None => std::future::pending().await,
    }
}

async fn send_message(State(state): State<FakeState>, body: String) -> Json<Value> {
    state.sent.send(form(&body)).unwrap();
    Json(success(json!({ "id": 1 })))
}

#[derive(Debug)]
struct Listener {
    handling: Handling,
    heard: mpsc::UnboundedSender<IncomingMessage>,
}

impl Actor for Listener {
    type Args = Self;
    type Error = Infallible;

    fn on_start(
        listener: Self,
        _: ActorRef<Self>,
    ) -> impl Future<Output = Result<Self, Self::Error>> + Send {
        std::future::ready(Ok(listener))
    }
}

impl Message<IncomingMessage> for Listener {
    type Reply = Handling;

    fn handle(
        &mut self,
        message: IncomingMessage,
        _: &mut Context<Self, Self::Reply>,
    ) -> impl Future<Output = Handling> + Send {
        self.heard.send(message).unwrap();
        std::future::ready(self.handling)
    }
}

struct FakeZulip {
    inbox: ActorRef<Inbox>,
    replies: mpsc::UnboundedSender<Reply>,
    registrations: mpsc::UnboundedReceiver<Form>,
    polls: mpsc::UnboundedReceiver<(String, String)>,
    sent: mpsc::UnboundedReceiver<Form>,
}

impl FakeZulip {
    async fn start() -> Self {
        let (replies, replies_out) = mpsc::unbounded_channel();
        let (registrations_in, registrations) = mpsc::unbounded_channel();
        let (polls_in, polls) = mpsc::unbounded_channel();
        let (sent_in, sent) = mpsc::unbounded_channel();
        let state = FakeState {
            replies: Arc::new(Mutex::new(replies_out)),
            registrations: registrations_in,
            registered: Arc::default(),
            polls: polls_in,
            sent: sent_in,
        };
        let router = Router::new()
            .route("/api/v1/users/me", get(own_user))
            .route("/api/v1/users/{user_id}", get(user))
            .route("/api/v1/register", post(register))
            .route("/api/v1/events", get(events))
            .route("/api/v1/messages", post(send_message))
            .with_state(state);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let settings = ZulipSettings {
            site: format!("http://{address}").parse().unwrap(),
            email: BotEmail::try_new("arboters-bot@chess.zulipchat.com").unwrap(),
            api_key: SecretString::from("key"),
            inbox: InboxSettings {
                initial_backoff: Duration::from_millis(10),
                max_backoff: Duration::from_millis(50),
            },
        };
        let messenger = ZulipMessenger::new(&settings).unwrap();
        let inbox = Inbox::spawn(Inbox::new(reqwest::Client::new(), &settings, messenger));
        Self {
            inbox,
            replies,
            registrations,
            polls,
            sent,
        }
    }

    async fn listening(handling: Handling) -> (Self, mpsc::UnboundedReceiver<IncomingMessage>) {
        let zulip = Self::start().await;
        let heard = zulip.listen(handling).await;
        (zulip, heard)
    }

    async fn listen(&self, handling: Handling) -> mpsc::UnboundedReceiver<IncomingMessage> {
        let (heard, hearing) = mpsc::unbounded_channel();
        let listener = Listener::spawn(Listener { handling, heard });
        self.inbox
            .ask(Listen(listener.reply_recipient()))
            .await
            .unwrap();
        hearing
    }

    fn reply(&self, status: StatusCode, body: Value) {
        self.replies.send((status, body)).unwrap();
    }

    fn deliver(&self, events: Value) {
        let mut body = success(json!({ "queue_id": "ignored" }));
        body["events"] = events;
        self.reply(StatusCode::OK, body);
    }

    async fn next_poll(&mut self) -> (String, String) {
        within_a_while(self.polls.recv()).await
    }
}

async fn within_a_while<T>(next: impl Future<Output = Option<T>>) -> T {
    tokio::time::timeout(Duration::from_secs(5), next)
        .await
        .unwrap()
        .unwrap()
}

fn direct_message(id: i64, sender: u64, content: &str) -> Value {
    group_message(id, sender, &[sender, BOT], content)
}

fn group_message(id: i64, sender: u64, recipients: &[u64], content: &str) -> Value {
    let recipients: Vec<Value> = recipients.iter().map(|id| json!({ "id": id })).collect();
    json!({
        "id": id,
        "type": "message",
        "flags": ["read"],
        "message": {
            "id": 1000 + id,
            "sender_id": sender,
            "content": content,
            "type": "private",
            "display_recipient": recipients,
            "subject": "",
        },
    })
}

fn channel_message(id: i64, sender: u64, content: &str, flags: &[&str]) -> Value {
    json!({
        "id": id,
        "type": "message",
        "flags": flags,
        "message": {
            "id": 1000 + id,
            "sender_id": sender,
            "content": content,
            "type": "stream",
            "display_recipient": "chess",
            "stream_id": 1,
            "subject": "games",
        },
    })
}

fn heard_from(
    sender: u64,
    role: Role,
    reply_to: NotificationTarget,
    text: &str,
) -> IncomingMessage {
    IncomingMessage {
        sender: ChatUserId(sender),
        role,
        reply_to,
        text: CommandText::new(text),
    }
}

fn dm_to(user_id: u64) -> NotificationTarget {
    NotificationTarget::DirectMessage {
        user_id: ChatUserId(user_id),
    }
}

fn games_topic() -> NotificationTarget {
    NotificationTarget::Channel {
        channel: ChannelName::try_new("chess").unwrap(),
        topic: TopicName::try_new("games").unwrap(),
    }
}

#[tokio::test]
async fn the_queue_is_registered_for_message_events_in_raw_markdown() {
    let (mut zulip, _) = FakeZulip::listening(Handling::Handled).await;

    let registration = within_a_while(zulip.registrations.recv()).await;

    assert_eq!(registration["event_types"], r#"["message"]"#);
    assert_eq!(registration["apply_markdown"], "false");
}

#[tokio::test]
async fn a_direct_message_is_handed_out_with_its_sender_role_and_conversation() {
    let (zulip, mut heard) = FakeZulip::listening(Handling::Handled).await;

    zulip.deliver(json!([direct_message(1, MODERATOR, " subscription list ")]));

    assert_eq!(
        within_a_while(heard.recv()).await,
        heard_from(
            MODERATOR,
            Role::Moderator,
            dm_to(MODERATOR),
            "subscription list"
        )
    );
}

#[tokio::test]
async fn a_channel_message_that_mentions_the_bot_is_handed_out_without_the_mention() {
    let (zulip, mut heard) = FakeZulip::listening(Handling::Handled).await;

    zulip.deliver(json!([channel_message(
        1,
        MEMBER,
        "@**Arboters** subscription list",
        &["mentioned"]
    )]));

    assert_eq!(
        within_a_while(heard.recv()).await,
        heard_from(MEMBER, Role::Member, games_topic(), "subscription list")
    );
}

#[tokio::test]
async fn a_mention_that_names_the_bot_by_id_is_stripped_too() {
    let (zulip, mut heard) = FakeZulip::listening(Handling::Handled).await;

    zulip.deliver(json!([channel_message(
        1,
        MEMBER,
        "@**Arboters|99** help",
        &["mentioned"]
    )]));

    assert_eq!(within_a_while(heard.recv()).await.text.as_str(), "help");
}

#[tokio::test]
async fn messages_not_addressed_to_the_bot_alone_are_ignored() {
    let (zulip, mut heard) = FakeZulip::listening(Handling::Handled).await;

    zulip.deliver(json!([
        channel_message(1, MEMBER, "@**Arboters** help", &["read"]),
        channel_message(2, MEMBER, "@**all** help", &["wildcard_mentioned"]),
        group_message(3, BOT, &[BOT, MEMBER], "I don't know that command."),
        channel_message(4, BOT, "@**Arboters** help", &["mentioned"]),
        group_message(5, MEMBER, &[MEMBER, MODERATOR, BOT], "help"),
        direct_message(6, MEMBER, "subscription list"),
    ]));

    assert_eq!(
        within_a_while(heard.recv()).await.text.as_str(),
        "subscription list"
    );
}

#[tokio::test]
async fn an_expired_queue_is_registered_again() {
    let (mut zulip, mut heard) = FakeZulip::listening(Handling::Handled).await;
    assert_eq!(
        zulip.next_poll().await,
        ("queue-1".to_owned(), "-1".to_owned())
    );

    zulip.reply(
        StatusCode::BAD_REQUEST,
        json!({
            "result": "error",
            "code": "BAD_EVENT_QUEUE_ID",
            "msg": "Bad event queue ID: queue-1",
            "queue_id": "queue-1",
        }),
    );
    let after_expiry = zulip.next_poll().await;
    zulip.deliver(json!([direct_message(1, MEMBER, "help")]));

    assert_eq!(after_expiry, ("queue-2".to_owned(), "-1".to_owned()));
    assert_eq!(within_a_while(heard.recv()).await.text.as_str(), "help");
}

#[tokio::test]
async fn a_failed_poll_is_retried_on_the_same_queue() {
    let (mut zulip, mut heard) = FakeZulip::listening(Handling::Handled).await;
    assert_eq!(
        zulip.next_poll().await,
        ("queue-1".to_owned(), "-1".to_owned())
    );

    zulip.reply(
        StatusCode::INTERNAL_SERVER_ERROR,
        json!({ "result": "error", "msg": "Internal server error" }),
    );
    let after_failure = zulip.next_poll().await;
    zulip.deliver(json!([direct_message(1, MEMBER, "help")]));

    assert_eq!(after_failure, ("queue-1".to_owned(), "-1".to_owned()));
    assert_eq!(within_a_while(heard.recv()).await.text.as_str(), "help");
}

#[tokio::test]
async fn heartbeats_are_acknowledged_and_the_poll_continues() {
    let (mut zulip, mut heard) = FakeZulip::listening(Handling::Handled).await;
    assert_eq!(
        zulip.next_poll().await,
        ("queue-1".to_owned(), "-1".to_owned())
    );

    zulip.deliver(json!([{ "id": 3, "type": "heartbeat" }]));
    let after_heartbeat = zulip.next_poll().await;
    zulip.deliver(json!([direct_message(4, MEMBER, "help")]));
    let after_message = zulip.next_poll().await;

    assert_eq!(after_heartbeat, ("queue-1".to_owned(), "3".to_owned()));
    assert_eq!(after_message, ("queue-1".to_owned(), "4".to_owned()));
    assert_eq!(within_a_while(heard.recv()).await.text.as_str(), "help");
}

#[tokio::test]
async fn a_message_no_listener_recognizes_gets_a_reply_pointing_at_help() {
    let (mut zulip, _heard) = FakeZulip::listening(Handling::Unrecognized).await;

    zulip.deliver(json!([direct_message(1, MEMBER, "hello")]));

    let reply = within_a_while(zulip.sent.recv()).await;
    assert_eq!(reply["type"], "direct");
    assert_eq!(reply["to"], "[10]");
    assert_eq!(reply["content"], UNRECOGNIZED);
}

#[tokio::test]
async fn a_message_any_listener_handles_gets_no_extra_reply() {
    let (mut zulip, mut ignoring) = FakeZulip::listening(Handling::Unrecognized).await;
    let mut handling = zulip.listen(Handling::Handled).await;

    zulip.deliver(json!([direct_message(1, MEMBER, "help")]));
    within_a_while(ignoring.recv()).await;
    within_a_while(handling.recv()).await;
    zulip.listen(Handling::Handled).await;

    assert!(zulip.sent.try_recv().is_err());
}
