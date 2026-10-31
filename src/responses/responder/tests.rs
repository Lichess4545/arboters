use std::collections::BTreeSet;
use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use kameo::actor::Spawn;
use rand::SeedableRng;
use sqlx::PgPool;

use super::*;
use crate::chat::{ChannelName, ChatUserId, DeliveryError, MessageId, MessageText, TopicName};
use crate::responses::{Matching, NewResponse, Phrase, PostgresResponses};

const MEMBER: ChatUserId = ChatUserId(10);
const MODERATOR: ChatUserId = ChatUserId(11);

#[derive(Clone, Debug, Default)]
struct RecordingMessenger {
    deliveries: Arc<Mutex<Vec<(Conversation, OutgoingMessage)>>>,
}

impl Messenger for RecordingMessenger {
    fn send(
        &self,
        conversation: &Conversation,
        message: &OutgoingMessage,
    ) -> impl Future<Output = Result<(), DeliveryError>> + Send {
        self.deliveries
            .lock()
            .unwrap()
            .push((conversation.clone(), message.clone()));
        std::future::ready(Ok(()))
    }
}

#[derive(Debug, Default)]
struct FakeInbox;

impl Actor for FakeInbox {
    type Args = Self;
    type Error = Infallible;

    fn on_start(
        inbox: Self,
        _: ActorRef<Self>,
    ) -> impl Future<Output = Result<Self, Self::Error>> + Send {
        std::future::ready(Ok(inbox))
    }
}

impl Message<Overhear> for FakeInbox {
    type Reply = ();

    fn handle(
        &mut self,
        _: Overhear,
        _: &mut Context<Self, Self::Reply>,
    ) -> impl Future<Output = ()> + Send {
        std::future::ready(())
    }
}

struct Chat {
    responder: ActorRef<Responder<PostgresResponses, RecordingMessenger>>,
    responses: PostgresResponses,
    messenger: RecordingMessenger,
}

fn topic(channel: &str, topic: &str) -> Conversation {
    Conversation::Topic {
        channel: ChannelName::try_new(channel).unwrap(),
        topic: TopicName::try_new(topic).unwrap(),
    }
}

impl Chat {
    fn start(pool: PgPool, cooldown: Duration) -> Self {
        let responses = PostgresResponses::new(pool);
        let messenger = RecordingMessenger::default();
        let inbox = FakeInbox::spawn(FakeInbox);
        let responder = Responder::spawn((
            Responder::new(
                responses.clone(),
                messenger.clone(),
                &ResponsesSettings { cooldown },
                StdRng::seed_from_u64(4545),
            ),
            inbox.recipient(),
        ));
        Self {
            responder,
            responses,
            messenger,
        }
    }

    async fn save(&self, phrase: &str, source: u64, markdown: &str) {
        self.save_matching(phrase, Matching::AnyCase, source, markdown)
            .await;
    }

    async fn save_exact(&self, phrase: &str, source: u64, markdown: &str) {
        self.save_matching(phrase, Matching::ExactCase, source, markdown)
            .await;
    }

    async fn save_matching(&self, phrase: &str, matching: Matching, source: u64, markdown: &str) {
        self.responses
            .add(NewResponse {
                phrase: Phrase::try_new(phrase).unwrap(),
                matching,
                markdown: MessageText::new(markdown.to_owned()),
                source: MessageId(source),
                added_by: MODERATOR,
            })
            .await
            .unwrap();
    }

    async fn say(&self, channel: &str, topic: &str, text: &str) {
        self.responder
            .ask(OverheardMessage {
                sender: MEMBER,
                channel: ChannelName::try_new(channel).unwrap(),
                topic: TopicName::try_new(topic).unwrap(),
                text: MessageText::new(text.to_owned()),
            })
            .await
            .unwrap();
    }

    fn replies(&self) -> Vec<(Conversation, String)> {
        self.messenger
            .deliveries
            .lock()
            .unwrap()
            .iter()
            .map(|(conversation, message)| (conversation.clone(), message.markdown().to_owned()))
            .collect()
    }
}

#[sqlx::test]
async fn a_phrase_in_a_channel_message_is_answered_in_the_same_topic(pool: PgPool) {
    let chat = Chat::start(pool, Duration::ZERO);
    chat.save("gg", 1, "Good game!").await;

    chat.say("chess", "games", "That was close. GG!").await;

    assert_eq!(
        chat.replies(),
        [(topic("chess", "games"), "Good game!".to_owned())]
    );
}

#[sqlx::test]
async fn a_message_without_any_phrase_as_whole_words_is_not_answered(pool: PgPool) {
    let chat = Chat::start(pool, Duration::ZERO);
    chat.save("gg", 1, "Good game!").await;

    chat.say("chess", "games", "I like eggs").await;

    assert_eq!(chat.replies(), []);
}

#[sqlx::test]
async fn each_phrase_in_a_message_is_answered(pool: PgPool) {
    let chat = Chat::start(pool, Duration::ZERO);
    chat.save("gg", 1, "Good game!").await;
    chat.save("rules", 2, "Read the rules.").await;

    chat.say("chess", "games", "gg, and what are the rules")
        .await;

    let answers: BTreeSet<String> = chat.replies().into_iter().map(|(_, text)| text).collect();
    assert_eq!(
        answers,
        BTreeSet::from(["Good game!".to_owned(), "Read the rules.".to_owned()])
    );
}

#[sqlx::test]
async fn a_phrase_with_several_responses_is_answered_with_any_of_them(pool: PgPool) {
    let chat = Chat::start(pool, Duration::ZERO);
    chat.save("gg", 1, "Good game!").await;
    chat.save("gg", 2, "Well played!").await;
    chat.save("rules", 3, "Read the rules.").await;

    for _ in 0..20 {
        chat.say("chess", "games", "gg").await;
    }

    let replies = chat.replies();
    let answers: BTreeSet<&str> = replies.iter().map(|(_, text)| text.as_str()).collect();
    assert_eq!(replies.len(), 20);
    assert_eq!(answers, BTreeSet::from(["Good game!", "Well played!"]));
}

#[sqlx::test]
async fn a_phrase_waits_out_its_cooldown_in_one_topic_but_not_in_others(pool: PgPool) {
    let chat = Chat::start(pool, Duration::from_hours(1));
    chat.save("gg", 1, "Good game!").await;

    chat.say("chess", "games", "gg").await;
    chat.say("chess", "games", "gg again").await;
    chat.say("chess", "round 2", "gg").await;

    assert_eq!(
        chat.replies(),
        [
            (topic("chess", "games"), "Good game!".to_owned()),
            (topic("chess", "round 2"), "Good game!".to_owned()),
        ]
    );
}

#[sqlx::test]
async fn an_exact_phrase_is_answered_only_in_its_own_case(pool: PgPool) {
    let chat = Chat::start(pool, Duration::ZERO);
    chat.save_exact("GG", 1, "GG WP!").await;

    chat.say("chess", "games", "gg").await;
    chat.say("chess", "games", "Gg").await;
    chat.say("chess", "games", "GG!").await;

    assert_eq!(
        chat.replies(),
        [(topic("chess", "games"), "GG WP!".to_owned())]
    );
}

#[sqlx::test]
async fn a_message_is_answered_once_from_every_response_its_case_matches(pool: PgPool) {
    let chat = Chat::start(pool, Duration::ZERO);
    chat.save("gg", 1, "Good game!").await;
    chat.save_exact("GG", 2, "GG WP!").await;

    for _ in 0..20 {
        chat.say("chess", "games", "GG").await;
    }
    for _ in 0..20 {
        chat.say("chess", "games", "gg").await;
    }

    let replies = chat.replies();
    assert_eq!(replies.len(), 40);
    let (shouted, quiet) = replies.split_at(20);
    let shouted: BTreeSet<&str> = shouted.iter().map(|(_, text)| text.as_str()).collect();
    let quiet: BTreeSet<&str> = quiet.iter().map(|(_, text)| text.as_str()).collect();
    assert_eq!(shouted, BTreeSet::from(["Good game!", "GG WP!"]));
    assert_eq!(quiet, BTreeSet::from(["Good game!"]));
}

#[sqlx::test]
async fn a_phrase_in_another_case_waits_out_the_same_cooldown(pool: PgPool) {
    let chat = Chat::start(pool, Duration::from_hours(1));
    chat.save("gg", 1, "Good game!").await;
    chat.save_exact("GG", 2, "GG WP!").await;

    chat.say("chess", "games", "gg").await;
    chat.say("chess", "games", "GG").await;

    assert_eq!(
        chat.replies(),
        [(topic("chess", "games"), "Good game!".to_owned())]
    );
}
