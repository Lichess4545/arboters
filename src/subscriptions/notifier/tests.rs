use std::convert::Infallible;
use std::sync::{Arc, Mutex};

use kameo::actor::Spawn;
use sqlx::PgPool;

use super::*;
use crate::chat::{
    ChannelName, ChatUserId, Conversation, DeliveryError, OutgoingMessage, TopicName,
};
use crate::game_monitor::{GameEventKind, LichessSettings};
use crate::heltour::{Pairing, league};
use crate::lichess::{self, Game, GameId, Player, Speed, Variant};
use crate::subscriptions::{NewSubscription, PostgresSubscriptions};

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

fn league(tag: &str) -> league::Tag {
    league::Tag::try_new(tag).unwrap()
}

fn user_id(name: &str) -> lichess::UserId {
    lichess::UserId::try_new(name).unwrap()
}

fn player(name: &str) -> Player {
    Player {
        user_id: user_id(name),
        rating: None,
    }
}

fn game() -> Game {
    Game {
        id: GameId::new("abcd1234"),
        white: player("alice"),
        black: player("bob"),
        speed: Speed::Rapid,
        rated: true,
        variant: Variant::Standard,
        clock: None,
    }
}

fn paired_in(tag: &str, white: &str, black: &str) -> ActiveLeagues {
    ActiveLeagues {
        leagues: Vec::new(),
        pairings: vec![Pairing {
            league: league(tag),
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

fn subscription(
    tag: &str,
    source: &str,
    event: GameEventKind,
    target: Conversation,
) -> NewSubscription {
    NewSubscription {
        requester: ChatUserId(1),
        league: league(tag),
        source: SubscriptionSource::Player(user_id(source)),
        event,
        target,
    }
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
            LichessSettings::default().base_url,
        ),
        monitor.recipient(),
    ));
    notifier.ask(active).await.unwrap();
    notifier.ask(GameEvent::Started(game())).await.unwrap();
    messenger.deliveries()
}

#[sqlx::test]
async fn subscribers_of_both_players_to_the_event_receive_it_rendered(pool: PgPool) {
    let notification = render(
        &GameEvent::Started(game()),
        &LichessSettings::default().base_url,
    );
    assert_eq!(
        deliveries(
            pool,
            [
                subscription(
                    "classical",
                    "alice",
                    GameEventKind::GameStarted,
                    direct_message(7)
                ),
                subscription(
                    "classical",
                    "bob",
                    GameEventKind::GameStarted,
                    direct_message(8)
                ),
                subscription(
                    "classical",
                    "alice",
                    GameEventKind::GameFinished,
                    direct_message(9)
                ),
            ],
            paired_in("classical", "alice", "bob"),
            RecordingMessenger::default(),
        )
        .await,
        vec![
            (direct_message(7), notification.clone()),
            (direct_message(8), notification),
        ]
    );
}

#[sqlx::test]
async fn a_game_is_announced_only_in_the_league_that_pairs_its_players(pool: PgPool) {
    assert_eq!(
        deliveries(
            pool,
            [
                subscription(
                    "classical",
                    "alice",
                    GameEventKind::GameStarted,
                    direct_message(7)
                ),
                subscription(
                    "rapid",
                    "alice",
                    GameEventKind::GameStarted,
                    direct_message(8)
                ),
            ],
            paired_in("rapid", "bob", "alice"),
            RecordingMessenger::default(),
        )
        .await,
        vec![(
            direct_message(8),
            render(
                &GameEvent::Started(game()),
                &LichessSettings::default().base_url
            )
        )]
    );
}

#[sqlx::test]
async fn a_failed_delivery_does_not_stop_delivery_to_the_remaining_targets(pool: PgPool) {
    assert_eq!(
        deliveries(
            pool,
            [
                subscription(
                    "classical",
                    "alice",
                    GameEventKind::GameStarted,
                    direct_message(7)
                ),
                subscription(
                    "classical",
                    "alice",
                    GameEventKind::GameStarted,
                    direct_message(8)
                ),
            ],
            paired_in("classical", "alice", "bob"),
            RecordingMessenger {
                unreachable: Some(direct_message(7)),
                ..RecordingMessenger::default()
            },
        )
        .await,
        vec![(
            direct_message(8),
            render(
                &GameEvent::Started(game()),
                &LichessSettings::default().base_url
            )
        )]
    );
}

fn games_topic() -> Conversation {
    Conversation::Topic {
        channel: ChannelName::try_new("chess").unwrap(),
        topic: TopicName::try_new("games").unwrap(),
    }
}

fn league_wide(tag: &str, target: Conversation) -> NewSubscription {
    NewSubscription {
        source: SubscriptionSource::League,
        ..subscription(tag, "alice", GameEventKind::GameStarted, target)
    }
}

#[sqlx::test]
async fn league_subscribers_hear_once_about_every_game_their_league_pairs(pool: PgPool) {
    let notification = render(
        &GameEvent::Started(game()),
        &LichessSettings::default().base_url,
    );
    assert_eq!(
        deliveries(
            pool,
            [
                league_wide("classical", direct_message(7)),
                league_wide("rapid", direct_message(8)),
                league_wide("classical", games_topic()),
                subscription(
                    "classical",
                    "alice",
                    GameEventKind::GameStarted,
                    games_topic()
                ),
            ],
            paired_in("classical", "alice", "bob"),
            RecordingMessenger::default(),
        )
        .await,
        vec![
            (direct_message(7), notification.clone()),
            (games_topic(), notification),
        ]
    );
}
