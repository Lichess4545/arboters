use std::sync::{Arc, Mutex};

use kameo::actor::Spawn;
use sqlx::PgPool;

use super::*;
use crate::chat::{
    ChannelName, ChatUserId, DeliveryError, Notification, NotificationTarget, TopicName,
};
use crate::game_monitor::GameEventKind;
use crate::heltour::{Pairing, league};
use crate::lichess::{self, Game, GameId, Player, Speed};
use crate::subscriptions::{NewSubscription, PostgresSubscriptions};

#[derive(Clone, Debug, Default)]
struct RecordingMessenger {
    deliveries: Arc<Mutex<Vec<(NotificationTarget, Notification)>>>,
    unreachable: Option<NotificationTarget>,
}

#[derive(Debug, thiserror::Error)]
#[error("unreachable")]
struct Unreachable;

impl RecordingMessenger {
    fn deliveries(&self) -> Vec<(NotificationTarget, Notification)> {
        self.deliveries.lock().unwrap().clone()
    }
}

impl Messenger for RecordingMessenger {
    fn send(
        &self,
        target: &NotificationTarget,
        notification: &Notification,
    ) -> impl Future<Output = Result<(), DeliveryError>> + Send {
        let outcome = if self.unreachable.as_ref() == Some(target) {
            Err(DeliveryError {
                target: target.clone(),
                cause: Box::new(Unreachable),
            })
        } else {
            self.deliveries
                .lock()
                .unwrap()
                .push((target.clone(), notification.clone()));
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

fn direct_message(user_id: u64) -> NotificationTarget {
    NotificationTarget::DirectMessage {
        user_id: ChatUserId(user_id),
    }
}

fn subscription(
    tag: &str,
    source: &str,
    event: GameEventKind,
    target: NotificationTarget,
) -> NewSubscription {
    NewSubscription {
        requester: ChatUserId(1),
        league: league(tag),
        source: SubscriptionSource::Player(user_id(source)),
        event,
        target,
    }
}

async fn deliveries(
    pool: PgPool,
    subscriptions: impl IntoIterator<Item = NewSubscription>,
    active: ActiveLeagues,
    messenger: RecordingMessenger,
) -> Vec<(NotificationTarget, Notification)> {
    let stored = PostgresSubscriptions::new(pool);
    for subscription in subscriptions {
        stored.subscribe(subscription).await.unwrap();
    }
    let notifier = Notifier::spawn(Notifier::new(stored, messenger.clone()));
    notifier.ask(active).await.unwrap();
    notifier.ask(GameEvent::Started(game())).await.unwrap();
    messenger.deliveries()
}

#[sqlx::test]
async fn subscribers_of_both_players_to_the_event_receive_it_rendered(pool: PgPool) {
    let notification = render(&GameEvent::Started(game()));
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
        vec![(direct_message(8), render(&GameEvent::Started(game())))]
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
        vec![(direct_message(8), render(&GameEvent::Started(game())))]
    );
}

fn games_topic() -> NotificationTarget {
    NotificationTarget::Channel {
        channel: ChannelName::try_new("chess").unwrap(),
        topic: TopicName::try_new("games").unwrap(),
    }
}

fn league_wide(tag: &str, target: NotificationTarget) -> NewSubscription {
    NewSubscription {
        source: SubscriptionSource::League,
        ..subscription(tag, "alice", GameEventKind::GameStarted, target)
    }
}

#[sqlx::test]
async fn league_subscribers_hear_once_about_every_game_their_league_pairs(pool: PgPool) {
    let notification = render(&GameEvent::Started(game()));
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
