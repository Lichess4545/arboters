use std::convert::Infallible;
use std::sync::{Arc, Mutex};

use kameo::actor::Spawn;
use sqlx::PgPool;

use super::*;
use crate::chat::{
    ChannelName, ChatUserId, DeliveryError, Notification, NotificationTarget, TopicName,
};
use crate::game_monitor::GameEventKind;
use crate::heltour::team_pairing::{Board, BoardNumber, HalfPoints};
use crate::heltour::{Pairing, TeamPairing, league, team};
use crate::lichess::{self, Game, GameId, Player, Speed, Variant};
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
        team_pairings: Vec::new(),
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
) -> Vec<(NotificationTarget, Notification)> {
    let stored = PostgresSubscriptions::new(pool);
    for subscription in subscriptions {
        stored.subscribe(subscription).await.unwrap();
    }
    let monitor = Monitor::spawn(Monitor);
    let notifier = Notifier::spawn((
        Notifier::new(stored, messenger.clone()),
        monitor.recipient(),
    ));
    notifier.ask(active).await.unwrap();
    notifier.ask(GameEvent::Started(game())).await.unwrap();
    messenger.deliveries()
}

#[sqlx::test]
async fn subscribers_of_both_players_to_the_event_receive_it_rendered(pool: PgPool) {
    let notification = render(&GameEvent::Started(game()), None);
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
        vec![(direct_message(8), render(&GameEvent::Started(game()), None))]
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
        vec![(direct_message(8), render(&GameEvent::Started(game()), None))]
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
    let notification = render(&GameEvent::Started(game()), None);
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

#[sqlx::test]
async fn a_team_game_is_announced_with_its_match_context(pool: PgPool) {
    let team = |name: &str| team::DisplayName::try_new(name).unwrap();
    let active = ActiveLeagues {
        team_pairings: vec![TeamPairing {
            league: league("team4545"),
            white_team: team("Knights"),
            white_points: HalfPoints::new(4),
            black_team: team("Rooks"),
            black_points: HalfPoints::new(4),
            boards: vec![Board {
                number: BoardNumber::new(5),
                white: Some(user_id("bob")),
                black: Some(user_id("alice")),
                white_team: team("Rooks"),
                black_team: team("Knights"),
                game_link: None,
                result: None,
            }],
        }],
        ..paired_in("team4545", "alice", "bob")
    };
    assert_eq!(
        deliveries(
            pool,
            [league_wide("team4545", games_topic())],
            active,
            RecordingMessenger::default(),
        )
        .await,
        vec![(
            games_topic(),
            Notification::new(
                "@_**alice** vs @_**bob** has started: https://lichess.org/game/abcd1234\nKnights and Rooks are tied 2–2. This game can decide the match.".to_owned()
            )
        )]
    );
}
