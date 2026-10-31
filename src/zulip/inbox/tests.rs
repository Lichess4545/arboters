use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use kameo::actor::Spawn;
use secrecy::SecretString;
use serde_json::{Value, json};
use sqlx::PgPool;
use tokio::net::TcpListener;
use tokio::sync::{Mutex, mpsc};

use super::*;
use crate::chat::{ChannelName, Role, TopicName};
use crate::game_monitor::{GameEventKind, LichessSettings};
use crate::heltour::{ActiveLeagues, League, league};
use crate::lichess::Variant;
use crate::subscriptions::{PostgresSubscriptions, SubscriptionCommands, Subscriptions};
use crate::zulip::BotEmail;

const BOT: u64 = 99;
const MEMBER: u64 = 10;
const MODERATOR: u64 = 11;
const ANSWER: &str = "Subscription `1`: tell you when a-game-starts in classical.";

fn answering() -> Handling {
    Handling::Answered(OutgoingMessage::new(ANSWER.to_owned()))
}

type Form = BTreeMap<String, String>;
type Reply = (StatusCode, Value);

#[derive(Clone)]
struct FakeState {
    replies: Arc<Mutex<mpsc::UnboundedReceiver<Reply>>>,
    registrations: mpsc::UnboundedSender<Form>,
    registered: Arc<std::sync::Mutex<u32>>,
    polls: mpsc::UnboundedSender<(String, String)>,
    sent: mpsc::UnboundedSender<Form>,
    sent_ids: Arc<AtomicU64>,
    edits: mpsc::UnboundedSender<(u64, Form)>,
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

async fn members() -> Json<Value> {
    let member = |user_id: u64, full_name: &str, is_active: bool| json!({ "user_id": user_id, "full_name": full_name, "is_active": is_active, "is_bot": false });
    Json(success(json!({
        "members": [
            member(23, "Alice Smith", true),
            member(24, "Bob Jones", true),
            member(25, "Bob Jones", true),
            member(26, "Carol Old", false),
        ],
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
    let id = state.sent_ids.fetch_add(1, Ordering::Relaxed);
    Json(success(json!({ "id": id })))
}

async fn edit_message(
    State(state): State<FakeState>,
    Path(message_id): Path<u64>,
    body: String,
) -> Json<Value> {
    state.edits.send((message_id, form(&body))).unwrap();
    Json(success(json!({})))
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
        std::future::ready(self.handling.clone())
    }
}

#[derive(Debug)]
struct Overhearer {
    overheard: mpsc::UnboundedSender<OverheardMessage>,
}

impl Actor for Overhearer {
    type Args = Self;
    type Error = Infallible;

    fn on_start(
        overhearer: Self,
        _: ActorRef<Self>,
    ) -> impl Future<Output = Result<Self, Self::Error>> + Send {
        std::future::ready(Ok(overhearer))
    }
}

impl Message<OverheardMessage> for Overhearer {
    type Reply = ();

    fn handle(
        &mut self,
        message: OverheardMessage,
        _: &mut Context<Self, Self::Reply>,
    ) -> impl Future<Output = ()> + Send {
        self.overheard.send(message).unwrap();
        std::future::ready(())
    }
}

struct FakeZulip {
    inbox: ActorRef<Inbox>,
    replies: mpsc::UnboundedSender<Reply>,
    registrations: mpsc::UnboundedReceiver<Form>,
    polls: mpsc::UnboundedReceiver<(String, String)>,
    sent: mpsc::UnboundedReceiver<Form>,
    edits: mpsc::UnboundedReceiver<(u64, Form)>,
}

impl FakeZulip {
    async fn start() -> Self {
        let (replies, replies_out) = mpsc::unbounded_channel();
        let (registrations_in, registrations) = mpsc::unbounded_channel();
        let (polls_in, polls) = mpsc::unbounded_channel();
        let (sent_in, sent) = mpsc::unbounded_channel();
        let (edits_in, edits) = mpsc::unbounded_channel();
        let state = FakeState {
            replies: Arc::new(Mutex::new(replies_out)),
            registrations: registrations_in,
            registered: Arc::default(),
            polls: polls_in,
            sent: sent_in,
            sent_ids: Arc::new(AtomicU64::new(5000)),
            edits: edits_in,
        };
        let router = Router::new()
            .route("/api/v1/users", get(members))
            .route("/api/v1/users/me", get(own_user))
            .route("/api/v1/users/{user_id}", get(user))
            .route("/api/v1/register", post(register))
            .route("/api/v1/events", get(events))
            .route("/api/v1/messages", post(send_message))
            .route("/api/v1/messages/{message_id}", patch(edit_message))
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
            edits,
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

    async fn overhear(&self) -> mpsc::UnboundedReceiver<OverheardMessage> {
        let (overheard, overhearing) = mpsc::unbounded_channel();
        let overhearer = Overhearer::spawn(Overhearer { overheard });
        self.inbox
            .ask(Overhear(overhearer.recipient()))
            .await
            .unwrap();
        overhearing
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

fn edit(id: i64, message_id: u64, sender: u64, content: &str, flags: &[&str]) -> Value {
    json!({
        "id": id,
        "type": "update_message",
        "user_id": sender,
        "rendering_only": false,
        "message_id": message_id,
        "message_ids": [message_id],
        "flags": flags,
        "edit_timestamp": 1_594_825_451,
        "orig_content": "earlier",
        "content": content,
        "is_me_message": false,
    })
}

fn channel_edit(id: i64, message_id: u64, sender: u64, content: &str, flags: &[&str]) -> Value {
    let mut edit = edit(id, message_id, sender, content, flags);
    edit["stream_id"] = json!(1);
    edit["stream_name"] = json!("chess");
    edit
}

fn topic_move(id: i64, message_id: u64, sender: u64) -> Value {
    json!({
        "id": id,
        "type": "update_message",
        "user_id": sender,
        "rendering_only": false,
        "message_id": message_id,
        "message_ids": [message_id],
        "flags": ["mentioned"],
        "edit_timestamp": 1_594_825_451,
        "stream_id": 1,
        "stream_name": "chess",
        "orig_subject": "games",
        "subject": "old games",
        "propagate_mode": "change_all",
    })
}

fn heard_from(
    event_id: u64,
    sender: u64,
    role: Role,
    reply_to: Conversation,
    text: &str,
) -> IncomingMessage {
    IncomingMessage {
        id: MessageId(1000 + event_id),
        revision: Revision::Original,
        sender: ChatUserId(sender),
        role,
        reply_to,
        text: CommandText::new(text),
    }
}

fn dm_to(user_id: u64) -> Conversation {
    Conversation::Direct {
        user_id: ChatUserId(user_id),
    }
}

fn games_topic() -> Conversation {
    Conversation::Topic {
        channel: ChannelName::try_new("chess").unwrap(),
        topic: TopicName::try_new("games").unwrap(),
    }
}

#[tokio::test]
async fn the_queue_is_registered_for_messages_and_their_edits_in_raw_markdown() {
    let (mut zulip, _) = FakeZulip::listening(answering()).await;

    let registration = within_a_while(zulip.registrations.recv()).await;

    assert_eq!(
        registration["event_types"],
        r#"["message","update_message"]"#
    );
    assert_eq!(registration["apply_markdown"], "false");
}

#[tokio::test]
async fn the_queue_is_registered_for_messages_in_every_public_channel() {
    let (mut zulip, _) = FakeZulip::listening(answering()).await;

    let registration = within_a_while(zulip.registrations.recv()).await;

    assert_eq!(registration["all_public_streams"], "true");
}

#[tokio::test]
async fn a_direct_message_is_handed_out_with_its_sender_role_and_conversation() {
    let (zulip, mut heard) = FakeZulip::listening(answering()).await;

    zulip.deliver(json!([direct_message(1, MODERATOR, " subscription list ")]));

    assert_eq!(
        within_a_while(heard.recv()).await,
        heard_from(
            1,
            MODERATOR,
            Role::Moderator,
            dm_to(MODERATOR),
            "subscription list"
        )
    );
}

#[tokio::test]
async fn a_channel_message_that_mentions_the_bot_is_handed_out_without_the_mention() {
    let (zulip, mut heard) = FakeZulip::listening(answering()).await;

    zulip.deliver(json!([channel_message(
        1,
        MEMBER,
        "@**Arboters** subscription list",
        &["mentioned"]
    )]));

    assert_eq!(
        within_a_while(heard.recv()).await,
        heard_from(1, MEMBER, Role::Member, games_topic(), "subscription list")
    );
}

#[tokio::test]
async fn a_mention_that_names_the_bot_by_id_is_stripped_too() {
    let (zulip, mut heard) = FakeZulip::listening(answering()).await;

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
    let (zulip, mut heard) = FakeZulip::listening(answering()).await;

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
    let (mut zulip, mut heard) = FakeZulip::listening(answering()).await;
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
    let (mut zulip, mut heard) = FakeZulip::listening(answering()).await;
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
    let (mut zulip, mut heard) = FakeZulip::listening(answering()).await;
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
async fn the_answers_of_every_listener_that_handles_a_message_are_one_reply() {
    let (mut zulip, mut ignoring) = FakeZulip::listening(Handling::Unrecognized).await;
    let _first = zulip.listen(answering()).await;
    let _second = zulip.listen(answering()).await;

    zulip.deliver(json!([direct_message(1, MEMBER, "help")]));
    within_a_while(ignoring.recv()).await;

    let reply = within_a_while(zulip.sent.recv()).await;
    assert_eq!(reply["content"], format!("{ANSWER}\n\n{ANSWER}"));
    zulip.listen(answering()).await;
    assert!(zulip.sent.try_recv().is_err());
}

#[tokio::test]
async fn an_edit_of_an_answered_message_is_handed_out_again_and_edits_the_reply() {
    let (mut zulip, mut heard) = FakeZulip::listening(answering()).await;
    zulip.deliver(json!([direct_message(1, MEMBER, "tell me")]));
    within_a_while(heard.recv()).await;
    within_a_while(zulip.sent.recv()).await;

    zulip.deliver(json!([edit(2, 1001, MEMBER, "help", &[])]));

    assert_eq!(
        within_a_while(heard.recv()).await,
        IncomingMessage {
            revision: Revision::Edited,
            ..heard_from(1, MEMBER, Role::Member, dm_to(MEMBER), "help")
        }
    );
    let (reply, edited) = within_a_while(zulip.edits.recv()).await;
    assert_eq!(reply, 5000);
    assert_eq!(edited["content"], ANSWER);
    assert!(zulip.sent.try_recv().is_err());
}

#[tokio::test]
async fn an_edit_of_an_unrecognized_message_edits_the_pointer_to_help() {
    let (mut zulip, mut heard) = FakeZulip::listening(Handling::Unrecognized).await;
    zulip.deliver(json!([channel_message(
        1,
        MEMBER,
        "@**Arboters** hello",
        &["mentioned"]
    )]));
    within_a_while(heard.recv()).await;
    within_a_while(zulip.sent.recv()).await;

    zulip.deliver(json!([channel_edit(
        2,
        1001,
        MEMBER,
        "@**Arboters** hi",
        &["mentioned"]
    )]));

    let edited = within_a_while(heard.recv()).await;
    assert_eq!(edited.text.as_str(), "hi");
    assert_eq!(edited.reply_to, games_topic());
    let (reply, edited) = within_a_while(zulip.edits.recv()).await;
    assert_eq!(reply, 5000);
    assert_eq!(edited["content"], UNRECOGNIZED);
}

#[tokio::test]
async fn topic_moves_unaddressed_edits_and_edits_of_unanswered_messages_are_ignored() {
    let (mut zulip, mut heard) = FakeZulip::listening(answering()).await;
    zulip.deliver(json!([channel_message(
        1,
        MEMBER,
        "@**Arboters** help",
        &["mentioned"]
    )]));
    within_a_while(heard.recv()).await;
    within_a_while(zulip.sent.recv()).await;

    zulip.deliver(json!([
        topic_move(2, 1001, MEMBER),
        channel_edit(3, 1001, MEMBER, "help", &["read"]),
        channel_edit(4, 1009, MEMBER, "@**Arboters** help", &["mentioned"]),
        {
            "id": 5,
            "type": "update_message",
            "user_id": null,
            "rendering_only": true,
            "message_id": 1001,
            "message_ids": [1001],
            "flags": ["mentioned"],
            "content": "@**Arboters** help",
        },
        direct_message(6, MEMBER, "subscription list"),
    ]));

    assert_eq!(
        within_a_while(heard.recv()).await.text.as_str(),
        "subscription list"
    );
    assert!(heard.try_recv().is_err());
    within_a_while(zulip.sent.recv()).await;
    assert!(zulip.edits.try_recv().is_err());
}

async fn answered_by_subscription_commands(pool: PgPool) -> (FakeZulip, PostgresSubscriptions) {
    let zulip = FakeZulip::start().await;
    let subscriptions = PostgresSubscriptions::new(pool);
    let commands = SubscriptionCommands::spawn((
        SubscriptionCommands::new(subscriptions.clone(), LichessSettings::default().base_url),
        zulip.inbox.clone().recipient(),
    ));
    commands
        .ask(ActiveLeagues {
            leagues: vec![League {
                tag: league::Tag::try_new("classical").unwrap(),
                name: league::Name::try_new("Classical League").unwrap(),
                time_control: None,
                variant: Variant::Standard,
                rated: true,
            }],
            pairings: Vec::new(),
        })
        .await
        .unwrap();
    (zulip, subscriptions)
}

#[sqlx::test]
async fn an_edit_of_a_refused_command_edits_the_refusal_into_the_new_answer(pool: PgPool) {
    let (mut zulip, subscriptions) = answered_by_subscription_commands(pool).await;
    zulip.deliver(json!([direct_message(
        1,
        MEMBER,
        "tell me when a game starts in clasical league"
    )]));
    let refusal = within_a_while(zulip.sent.recv()).await;

    zulip.deliver(json!([edit(
        2,
        1001,
        MEMBER,
        "tell me when a game starts in classical league",
        &[]
    )]));

    let (reply, edited) = within_a_while(zulip.edits.recv()).await;
    let made = subscriptions
        .requested_by(ChatUserId(MEMBER))
        .await
        .unwrap();
    assert!(
        refusal["content"].starts_with("`clasical league` is not an active league."),
        "{refusal:?}"
    );
    assert_eq!(reply, 5000);
    assert_eq!(
        edited["content"],
        format!(
            "Subscription `{}`: tell you when a-game-starts in classical.",
            made[0].id
        )
    );
}

#[sqlx::test]
async fn an_edit_into_another_subscription_replaces_the_one_it_made(pool: PgPool) {
    let (mut zulip, subscriptions) = answered_by_subscription_commands(pool).await;
    zulip.deliver(json!([direct_message(
        1,
        MEMBER,
        "tell me when a game starts in classical"
    )]));
    within_a_while(zulip.sent.recv()).await;
    let earlier = subscriptions
        .requested_by(ChatUserId(MEMBER))
        .await
        .unwrap()[0]
        .id;

    zulip.deliver(json!([edit(
        2,
        1001,
        MEMBER,
        "tell me when a game ends in classical",
        &[]
    )]));

    let (_, edited) = within_a_while(zulip.edits.recv()).await;
    let made = subscriptions
        .requested_by(ChatUserId(MEMBER))
        .await
        .unwrap();
    assert_eq!(made.len(), 1);
    assert_eq!(made[0].event, GameEventKind::GameFinished);
    assert_eq!(
        edited["content"],
        format!(
            "Subscription `{}`: tell you when a-game-is-over in classical. It replaces subscription `{earlier}`.",
            made[0].id
        )
    );
}

#[tokio::test]
async fn a_mention_by_name_alone_gets_the_id_of_the_one_active_user_with_that_name() {
    let (zulip, mut heard) = FakeZulip::listening(answering()).await;

    zulip.deliver(json!([
        direct_message(
            1,
            MEMBER,
            "tell @**alice smith** and @_**Alice Smith** when"
        ),
        direct_message(
            2,
            MEMBER,
            "tell @**Bob Jones** or @**Carol Old** or @**Dave**"
        ),
        direct_message(3, MEMBER, "tell @**Bob Jones|25** or @**all**"),
    ]));

    let mut texts = Vec::new();
    for _ in 0..3 {
        texts.push(within_a_while(heard.recv()).await.text.as_str().to_owned());
    }
    assert_eq!(
        texts,
        [
            "tell @**alice smith|23** and @_**Alice Smith|23** when",
            "tell @**Bob Jones** or @**Carol Old** or @**Dave**",
            "tell @**Bob Jones|25** or @**all**",
        ]
    );
}

#[tokio::test]
async fn an_ordinary_channel_message_is_overheard_and_never_handed_out_as_a_command() {
    let (zulip, mut heard) = FakeZulip::listening(answering()).await;
    let mut overheard = zulip.overhear().await;

    zulip.deliver(json!([
        channel_message(1, BOT, "gg", &[]),
        channel_message(2, MEMBER, "@**Arboters** help", &["mentioned"]),
        channel_message(3, MEMBER, "gg everyone", &["read"]),
        direct_message(4, MEMBER, "gg"),
    ]));

    assert_eq!(
        within_a_while(overheard.recv()).await,
        OverheardMessage {
            sender: ChatUserId(MEMBER),
            channel: ChannelName::try_new("chess").unwrap(),
            topic: TopicName::try_new("games").unwrap(),
            text: MessageText::new("gg everyone".to_owned()),
        }
    );
    assert_eq!(within_a_while(heard.recv()).await.text.as_str(), "help");
    assert_eq!(within_a_while(heard.recv()).await.text.as_str(), "gg");
}
