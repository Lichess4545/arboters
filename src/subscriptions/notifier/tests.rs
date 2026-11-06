use std::convert::Infallible;
use std::sync::{Arc, Mutex};

use kameo::actor::Spawn;
use sqlx::PgPool;

use super::*;
use crate::chat::{
    ChannelName, ChatUserId, Conversation, DeliveryError, OutgoingMessage, TopicName,
};
use crate::game_monitor::GameEventKind;
use crate::heltour::Pairing;
use crate::lichess;
use crate::subscriptions::{NewSubscription, PostgresSubscriptions, SubscriptionSource};
use crate::testing::{alice_vs_bob, tag, user_id};

#[derive(Clone, Debug, Default)]
struct RecordingMessenger {
    deliveries: Arc<Mutex<Vec<(Conversation, OutgoingMessage)>>>,
    unreachable: Option<Conversation>,
}

#[derive(Debug, thiserror::Error)]
#[error("unreachable")]
struct Unreachable;

impl RecordingMessenger {
    fn deliveries(&self) -> Vec<(Conversation, OutgoingMessage)> {
        self.deliveries.lock().unwrap().clone()
    }
}

impl Messenger for RecordingMessenger {
    fn send(
        &self,
        conversation: &Conversation,
        message: &OutgoingMessage,
    ) -> impl Future<Output = Result<(), DeliveryError>> + Send {
        let outcome = if self.unreachable.as_ref() == Some(conversation) {
            Err(DeliveryError {
                conversation: conversation.clone(),
                cause: Box::new(Unreachable),
            })
        } else {
            self.deliveries
                .lock()
                .unwrap()
                .push((conversation.clone(), message.clone()));
            Ok(())
        };
        std::future::ready(outcome)
    }
}

fn paired_in(league: &str, white: &str, black: &str) -> ActiveLeagues {
    ActiveLeagues {
        leagues: Vec::new(),
        pairings: vec![Pairing {
            league: tag(league),
            white: user_id(white),
            black: user_id(black),
        }],
    }
}

fn direct_message(user_id: u64) -> Conversation {
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

fn starts(league: &str, source: SubscriptionSource, target: Conversation) -> NewSubscription {
    NewSubscription {
        requester: ChatUserId(1),
        league: tag(league),
        source,
        event: GameEventKind::GameStarted,
        target,
    }
}

fn of(name: &str) -> SubscriptionSource {
    SubscriptionSource::Player(user_id(name))
}

fn announced(
    targets: impl IntoIterator<Item = Conversation>,
) -> Vec<(Conversation, OutgoingMessage)> {
    let notification = render(
        &GameEvent::Started(alice_vs_bob("abcd1234")),
        &lichess::Config::default().base_url,
    );
    targets
        .into_iter()
        .map(|target| (target, notification.clone()))
        .collect()
}

#[derive(Debug)]
struct Monitor;

impl Actor for Monitor {
    type Args = Self;
    type Error = Infallible;

    fn on_start(
        monitor: Self,
        _: ActorRef<Self>,
    ) -> impl Future<Output = Result<Self, Self::Error>> + Send {
        std::future::ready(Ok(monitor))
    }
}

impl Message<Follow<GameEvent>> for Monitor {
    type Reply = ();

    async fn handle(&mut self, _: Follow<GameEvent>, _: &mut Context<Self, Self::Reply>) {}
}

async fn deliveries(
    pool: PgPool,
    subscriptions: impl IntoIterator<Item = NewSubscription>,
    active: ActiveLeagues,
    messenger: RecordingMessenger,
) -> Vec<(Conversation, OutgoingMessage)> {
    let stored = PostgresSubscriptions::new(pool);
    for subscription in subscriptions {
        stored.subscribe(subscription).await.unwrap();
    }
    let monitor = Monitor::spawn(Monitor);
    let notifier = Notifier::spawn((
        Notifier::new(
            stored,
            messenger.clone(),
            lichess::Config::default().base_url,
        ),
        monitor.recipient(),
    ));
    notifier.ask(active).await.unwrap();
    notifier
        .ask(GameEvent::Started(alice_vs_bob("abcd1234")))
        .await
        .unwrap();
    messenger.deliveries()
}

#[sqlx::test]
async fn subscribers_of_both_players_to_the_event_receive_it_rendered(pool: PgPool) {
    let finishes = NewSubscription {
        event: GameEventKind::GameFinished,
        ..starts("classical", of("alice"), direct_message(9))
    };

    let delivered = deliveries(
        pool,
        [
            starts("classical", of("alice"), direct_message(7)),
            starts("classical", of("bob"), direct_message(8)),
            finishes,
        ],
        paired_in("classical", "alice", "bob"),
        RecordingMessenger::default(),
    )
    .await;

    assert_eq!(delivered, announced([direct_message(7), direct_message(8)]));
}

#[sqlx::test]
async fn a_game_is_announced_only_in_the_league_that_pairs_its_players(pool: PgPool) {
    let delivered = deliveries(
        pool,
        [
            starts("classical", of("alice"), direct_message(7)),
            starts("rapid", of("alice"), direct_message(8)),
        ],
        paired_in("rapid", "bob", "alice"),
        RecordingMessenger::default(),
    )
    .await;

    assert_eq!(delivered, announced([direct_message(8)]));
}

#[sqlx::test]
async fn a_failed_delivery_does_not_stop_delivery_to_the_remaining_targets(pool: PgPool) {
    let delivered = deliveries(
        pool,
        [
            starts("classical", of("alice"), direct_message(7)),
            starts("classical", of("alice"), direct_message(8)),
        ],
        paired_in("classical", "alice", "bob"),
        RecordingMessenger {
            unreachable: Some(direct_message(7)),
            ..RecordingMessenger::default()
        },
    )
    .await;

    assert_eq!(delivered, announced([direct_message(8)]));
}

#[sqlx::test]
async fn league_subscribers_hear_once_about_every_game_their_league_pairs(pool: PgPool) {
    let delivered = deliveries(
        pool,
        [
            starts("classical", SubscriptionSource::League, direct_message(7)),
            starts("rapid", SubscriptionSource::League, direct_message(8)),
            starts("classical", SubscriptionSource::League, games_topic()),
            starts("classical", of("alice"), games_topic()),
        ],
        paired_in("classical", "alice", "bob"),
        RecordingMessenger::default(),
    )
    .await;

    assert_eq!(delivered, announced([direct_message(7), games_topic()]));
}
