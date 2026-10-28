use std::sync::{Arc, Mutex};

use kameo::actor::Spawn;
use sqlx::PgPool;

use super::*;
use crate::game_monitor::{GameEventKind, GameOutcome};
use crate::lichess::{self, Game, GameId, Player, Speed};
use crate::notifications::{DeliveryError, Notification};
use crate::subscriptions::{
    ChatUserId, NewSubscription, NotificationTarget, PostgresSubscriptions,
};

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

fn league() -> league::Tag {
    league::Tag::try_new("team4545").unwrap()
}

fn player(name: &str) -> Player {
    Player {
        user_id: lichess::UserId::try_new(name).unwrap(),
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

fn direct_message(user_id: u64) -> NotificationTarget {
    NotificationTarget::DirectMessage {
        user_id: ChatUserId(user_id),
    }
}

fn subscription(source: &str, event: GameEventKind, target: NotificationTarget) -> NewSubscription {
    NewSubscription {
        requester: ChatUserId(1),
        league: league(),
        source: SubscriptionSource::Player(lichess::UserId::try_new(source).unwrap()),
        event,
        target,
    }
}

async fn subscribed(
    pool: PgPool,
    new_subscriptions: impl IntoIterator<Item = NewSubscription>,
) -> PostgresSubscriptions {
    let subscriptions = PostgresSubscriptions::new(pool);
    for subscription in new_subscriptions {
        subscriptions.subscribe(subscription).await.unwrap();
    }
    subscriptions
}

async fn deliver(notifier: Notifier<PostgresSubscriptions, RecordingMessenger>, event: GameEvent) {
    Notifier::spawn(notifier).ask(event).await.unwrap();
}

#[sqlx::test]
async fn subscribers_of_both_players_receive_the_rendered_event(pool: PgPool) {
    let subscriptions = subscribed(
        pool,
        [
            subscription("alice", GameEventKind::GameStarted, direct_message(7)),
            subscription("bob", GameEventKind::GameStarted, direct_message(8)),
        ],
    )
    .await;
    let event = GameEvent::Started(game());
    let messenger = RecordingMessenger::default();

    deliver(
        Notifier::new(league(), subscriptions, messenger.clone()),
        event.clone(),
    )
    .await;

    let notification = render(&event);
    assert_eq!(
        messenger.deliveries(),
        vec![
            (direct_message(7), notification.clone()),
            (direct_message(8), notification),
        ]
    );
}

#[sqlx::test]
async fn subscribers_to_another_event_kind_are_not_notified(pool: PgPool) {
    let subscriptions = subscribed(
        pool,
        [
            subscription("alice", GameEventKind::GameStarted, direct_message(7)),
            subscription("alice", GameEventKind::GameFinished, direct_message(8)),
        ],
    )
    .await;
    let messenger = RecordingMessenger::default();
    let event = GameEvent::Finished {
        game: game(),
        outcome: GameOutcome::Draw,
    };

    deliver(
        Notifier::new(league(), subscriptions, messenger.clone()),
        event.clone(),
    )
    .await;

    assert_eq!(
        messenger.deliveries(),
        vec![(direct_message(8), render(&event))]
    );
}

#[sqlx::test]
async fn subscribers_in_another_league_are_not_notified(pool: PgPool) {
    let lonewolf = league::Tag::try_new("lonewolf").unwrap();
    let subscriptions = subscribed(
        pool,
        [
            subscription("alice", GameEventKind::GameStarted, direct_message(7)),
            NewSubscription {
                league: lonewolf.clone(),
                ..subscription("alice", GameEventKind::GameStarted, direct_message(8))
            },
        ],
    )
    .await;
    let event = GameEvent::Started(game());
    let messenger = RecordingMessenger::default();

    deliver(
        Notifier::new(lonewolf, subscriptions, messenger.clone()),
        event.clone(),
    )
    .await;

    assert_eq!(
        messenger.deliveries(),
        vec![(direct_message(8), render(&event))]
    );
}

#[sqlx::test]
async fn a_failed_delivery_does_not_stop_delivery_to_the_remaining_targets(pool: PgPool) {
    let subscriptions = subscribed(
        pool,
        [
            subscription("alice", GameEventKind::GameStarted, direct_message(7)),
            subscription("alice", GameEventKind::GameStarted, direct_message(8)),
        ],
    )
    .await;
    let event = GameEvent::Started(game());
    let messenger = RecordingMessenger {
        unreachable: Some(direct_message(7)),
        ..RecordingMessenger::default()
    };

    deliver(
        Notifier::new(league(), subscriptions, messenger.clone()),
        event.clone(),
    )
    .await;

    assert_eq!(
        messenger.deliveries(),
        vec![(direct_message(8), render(&event))]
    );
}
