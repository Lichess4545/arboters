use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::Router;
use axum::body::Body;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use futures::channel::mpsc::{UnboundedSender, unbounded};
use kameo::actor::Spawn;
use secrecy::SecretString;
use sqlx::PgPool;
use tokio::net::TcpListener;
use tokio::sync::{broadcast, mpsc, oneshot};
use url::Url;

use super::*;
use crate::game_monitor::{
    GameChange, Period, PostgresFixturesStore, StreamConnected, http_client,
};
use crate::health::{Recovery, Unavailable};
use crate::lichess::{Game, GameId, GameStatus};
use crate::testing::{Collector, alice_vs_bob, fixture, league, player, user_id, user_ids};

const STARTED: &str = r#"{"id":"g1","rated":true,"variant":"standard","speed":"classical","status":20,"statusName":"started","players":{"white":{"userId":"alice"},"black":{"userId":"bob"}}}"#;

enum Answer {
    Status(StatusCode),
    Lines(&'static [&'static str]),
}

struct Request {
    user_ids: Vec<lichess::UserId>,
    at: Instant,
    lines: Option<UnboundedSender<Result<String, std::convert::Infallible>>>,
}

#[derive(Clone, Default)]
struct FakeLichess {
    requests: Arc<Mutex<Vec<Request>>>,
    answers: Arc<Mutex<VecDeque<Answer>>>,
}

impl FakeLichess {
    fn answer_next_with(&self, answer: Answer) {
        self.answers.lock().unwrap().push_back(answer);
    }

    fn answer(&self, body: &str) -> Response {
        let user_ids = body.split(',').map(user_id).collect();
        let at = Instant::now();
        let answer = self.answers.lock().unwrap().pop_front();
        let (lines, response) = match answer {
            Some(Answer::Status(status)) => (None, status.into_response()),
            Some(Answer::Lines(lines)) => {
                let body: String = lines.iter().flat_map(|line| [*line, "\n"]).collect();
                (None, body.into_response())
            }
            None => {
                let (sender, receiver) = unbounded();
                (Some(sender), Body::from_stream(receiver).into_response())
            }
        };
        self.requests.lock().unwrap().push(Request {
            user_ids,
            at,
            lines,
        });
        response
    }

    fn request_count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }

    fn connected_user_ids(&self) -> Vec<Vec<lichess::UserId>> {
        let mut connected: Vec<_> = self
            .requests
            .lock()
            .unwrap()
            .iter()
            .map(|request| request.user_ids.clone())
            .collect();
        connected.sort();
        connected
    }

    fn connection_sizes(&self) -> Vec<usize> {
        let mut sizes: Vec<_> = self
            .requests
            .lock()
            .unwrap()
            .iter()
            .map(|request| request.user_ids.len())
            .collect();
        sizes.sort_unstable();
        sizes
    }

    fn gap_before(&self, request: usize) -> Duration {
        let requests = self.requests.lock().unwrap();
        requests[request].at - requests[request - 1].at
    }

    fn stream_line(&self, request: usize, line: &str) -> bool {
        self.requests.lock().unwrap()[request]
            .lines
            .as_ref()
            .unwrap()
            .unbounded_send(Ok(format!("{line}\n")))
            .is_ok()
    }
}

struct Harness {
    lichess: FakeLichess,
    fixtures_store: PostgresFixturesStore,
    monitor: ActorRef<GameMonitor<PostgresFixturesStore>>,
    events: broadcast::Receiver<GameEvent>,
    connections: mpsc::UnboundedReceiver<StreamConnected>,
    health: watch::Sender<Option<Health>>,
}

impl Harness {
    async fn submit(&self, fixtures: NewFixtures) -> FixturesId {
        let (stored, id) = oneshot::channel();
        self.monitor
            .tell(SubmitFixtures { fixtures, stored })
            .await
            .unwrap();
        id.await.unwrap()
    }

    async fn watch(&self, names: &[impl AsRef<str> + Sync]) -> FixturesId {
        let names: Vec<&str> = names.iter().map(AsRef::as_ref).collect();
        self.submit(fixtures_among("team4545", &names)).await
    }

    async fn current(&self, keys: &[&str]) -> CurrentFixtures {
        let (found, current) = oneshot::channel();
        self.monitor
            .tell(FindCurrentFixtures {
                leagues: keys.iter().map(|key| league("heltour", key)).collect(),
                found,
            })
            .await
            .unwrap();
        current.await.unwrap()
    }

    async fn hand_over(&self, state: lichess::GameState) {
        self.monitor.tell(state).await.unwrap();
    }

    async fn active_ids(&self) -> Vec<FixturesId> {
        self.fixtures_store
            .active()
            .await
            .unwrap()
            .iter()
            .map(|fixtures| fixtures.id)
            .collect()
    }

    async fn await_requests(&self, count: usize) {
        eventually(&format!("lichess has {count} requests"), || {
            self.lichess.request_count() == count
        })
        .await;
    }

    async fn next_event(&mut self) -> GameEvent {
        next(&mut self.events).await
    }

    async fn next_connection(&mut self) -> StreamConnected {
        tokio::time::timeout(Duration::from_secs(5), self.connections.recv())
            .await
            .unwrap()
            .unwrap()
    }

    fn owned_by(&self, lichess: Option<Availability<TokenOwner>>) {
        self.health.send_replace(Some(Health {
            postgres: Availability::Available(()),
            heltour: Availability::Available(()),
            lichess,
            zulip: Availability::Available(()),
        }));
    }
}

async fn next(events: &mut broadcast::Receiver<GameEvent>) -> GameEvent {
    tokio::time::timeout(Duration::from_secs(5), events.recv())
        .await
        .unwrap()
        .unwrap()
}

async fn harness(pool: PgPool, stream: lichess::StreamConfig) -> Harness {
    harness_with_token(pool, None, stream).await
}

async fn harness_with_token(
    pool: PgPool,
    token: Option<&str>,
    stream: lichess::StreamConfig,
) -> Harness {
    let lichess = FakeLichess::default();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let answering = lichess.clone();
    let router = Router::new().route(
        "/api/stream/games-by-users",
        post(move |body: String| std::future::ready(answering.answer(&body))),
    );
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let config = lichess::Config {
        base_url: Url::parse(&format!("http://{address}")).unwrap(),
        token: token.map(SecretString::from),
        stream,
    };
    let (connection_sender, connections) = mpsc::unbounded_channel();
    let (health_sender, health) = watch::channel(None);
    let fixtures_store = PostgresFixturesStore::new(pool);
    let monitor = GameMonitor::new(
        lichess::Api::new(
            http_client().unwrap(),
            &config.base_url,
            config.token.clone(),
        ),
        config,
        fixtures_store.clone(),
        Collector::spawn(Collector(connection_sender)).recipient(),
    );
    let events = monitor.events();
    Harness {
        lichess,
        fixtures_store,
        monitor: GameMonitor::spawn((monitor, health)),
        events,
        connections,
        health: health_sender,
    }
}

fn config() -> lichess::StreamConfig {
    lichess::StreamConfig {
        initial_backoff: Duration::from_millis(10),
        max_backoff: Duration::from_secs(1),
        rate_limit_backoff: Duration::from_millis(10),
        ..lichess::StreamConfig::default()
    }
}

async fn eventually(what: &str, condition: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting until {what}");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

fn player_names(count: usize) -> Vec<String> {
    (0..count)
        .map(|number| format!("player{number:04}"))
        .collect()
}

fn fixtures_of(key: &str, games: &[(&str, &str)]) -> NewFixtures {
    NewFixtures {
        league: league("heltour", key),
        period: Period::default(),
        games: games
            .iter()
            .map(|(white, black)| fixture(white, black))
            .collect(),
    }
}

fn fixtures_among(key: &str, names: &[&str]) -> NewFixtures {
    let games: Vec<(&str, &str)> = names.windows(2).map(|pair| (pair[0], pair[1])).collect();
    fixtures_of(key, &games)
}

fn ending_in(new: NewFixtures, remaining: time::Duration) -> NewFixtures {
    NewFixtures {
        period: Period {
            starts: None,
            ends: Some(OffsetDateTime::now_utc() + remaining),
        },
        ..new
    }
}

fn matched(id: FixturesId, key: &str, white: &str, black: &str) -> MatchedFixture {
    MatchedFixture {
        fixtures_id: id,
        league: league("heltour", key),
        fixture: fixture(white, black),
    }
}

fn game_between(white: &str, black: &str) -> Game {
    Game {
        id: GameId::new(format!("{white}-{black}")),
        white: player(white),
        black: player(black),
        ..alice_vs_bob("g1")
    }
}

fn started(game: Game) -> lichess::GameState {
    lichess::GameState {
        game,
        status: GameStatus::Started,
    }
}

#[sqlx::test]
async fn unchanged_fixtures_keep_their_id_and_connection(pool: PgPool) {
    let harness = harness(pool, config()).await;
    let first = harness.watch(&["alice", "bob"]).await;
    harness.await_requests(1).await;

    assert_eq!(harness.watch(&["alice", "bob"]).await, first);

    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(harness.lichess.request_count(), 1);
    assert_eq!(harness.active_ids().await, [first]);
}

#[sqlx::test]
async fn a_new_period_replaces_the_active_fixtures(pool: PgPool) {
    let harness = harness(pool, config()).await;
    let games = fixtures_of("team4545", &[("alice", "bob")]);
    let first = harness.submit(games.clone()).await;

    let rescheduled = harness
        .submit(ending_in(games, time::Duration::days(7)))
        .await;

    assert_ne!(rescheduled, first);
    assert_eq!(harness.active_ids().await, [rescheduled]);
}

#[sqlx::test]
async fn new_games_replace_the_active_fixtures_and_reconnect(pool: PgPool) {
    let harness = harness(pool, config()).await;
    let replaced = harness.watch(&["alice", "bob"]).await;
    harness.await_requests(1).await;

    let replacement = harness.watch(&["alice", "carol"]).await;

    assert_ne!(replacement, replaced);
    assert_eq!(harness.active_ids().await, [replacement]);
    harness.await_requests(2).await;
    eventually("the old connection closes", || {
        !harness.lichess.stream_line(0, "")
    })
    .await;
    assert_eq!(
        harness.lichess.connected_user_ids(),
        [user_ids(&["alice", "bob"]), user_ids(&["alice", "carol"])]
    );
}

#[sqlx::test]
async fn every_leagues_fixtures_are_stored_and_watched(pool: PgPool) {
    let harness = harness(pool, config()).await;
    let team = harness
        .submit(fixtures_of("team4545", &[("alice", "bob")]))
        .await;
    let lone_wolf = harness
        .submit(fixtures_of("lonewolf", &[("bob", "carol")]))
        .await;

    assert_eq!(harness.active_ids().await, [team, lone_wolf]);
    eventually("both leagues are watched", || {
        harness.lichess.connected_user_ids().last() == Some(&user_ids(&["alice", "bob", "carol"]))
    })
    .await;
}

#[sqlx::test]
async fn stored_active_fixtures_are_watched_on_startup(pool: PgPool) {
    PostgresFixturesStore::new(pool.clone())
        .store(fixtures_of("team4545", &[("alice", "bob")]))
        .await
        .unwrap();

    let harness = harness(pool, config()).await;

    harness.await_requests(1).await;
    assert_eq!(
        harness.lichess.connected_user_ids(),
        [user_ids(&["alice", "bob"])]
    );
}

#[sqlx::test]
async fn ended_fixtures_are_completed_and_no_longer_watched(pool: PgPool) {
    let harness = harness(pool, config()).await;
    harness
        .submit(ending_in(
            fixtures_of("team4545", &[("alice", "bob")]),
            time::Duration::milliseconds(300),
        ))
        .await;
    let other = harness
        .submit(fixtures_of("lonewolf", &[("bob", "carol")]))
        .await;

    eventually("only bob and carol are watched", || {
        harness.lichess.connected_user_ids().last() == Some(&user_ids(&["bob", "carol"]))
    })
    .await;
    assert_eq!(harness.active_ids().await, [other]);
}

#[sqlx::test]
async fn only_asked_leagues_with_active_fixtures_are_found(pool: PgPool) {
    let harness = harness(pool, config()).await;
    let team = harness
        .submit(fixtures_of("team4545", &[("alice", "bob")]))
        .await;
    harness
        .submit(fixtures_of("lonewolf", &[("carol", "dave")]))
        .await;
    harness
        .submit(ending_in(
            fixtures_of("chess960", &[("erin", "frank")]),
            -time::Duration::minutes(1),
        ))
        .await;
    tokio::time::timeout(Duration::from_secs(5), async {
        while harness.current(&["chess960"]).await != CurrentFixtures::default() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the ended chess960 fixtures complete");

    assert_eq!(
        harness.current(&["team4545", "chess960", "blitz"]).await,
        CurrentFixtures(BTreeMap::from([(league("heltour", "team4545"), team)]))
    );
}

#[sqlx::test]
async fn an_event_matches_its_players_fixtures_in_either_colour(pool: PgPool) {
    let mut harness = harness(pool, config()).await;
    let team = harness
        .submit(fixtures_of("team4545", &[("alice", "bob")]))
        .await;
    let lone_wolf = harness
        .submit(fixtures_of(
            "lonewolf",
            &[("bob", "alice"), ("alice", "carol")],
        ))
        .await;
    harness.await_requests(2).await;

    harness.lichess.stream_line(1, STARTED);
    assert_eq!(
        harness.next_event().await,
        GameEvent {
            change: GameChange::Started(alice_vs_bob("g1")),
            matched: vec![
                matched(lone_wolf, "lonewolf", "bob", "alice"),
                matched(team, "team4545", "alice", "bob"),
            ],
        }
    );

    harness
        .hand_over(started(game_between("bob", "carol")))
        .await;
    assert_eq!(harness.next_event().await.matched, []);
}

#[sqlx::test]
async fn the_token_owner_decides_how_players_are_split(pool: PgPool) {
    let harness = harness_with_token(pool, Some("lip_secret"), config()).await;
    let names = player_names(901);
    harness.watch(&names).await;
    harness.await_requests(2).await;

    harness.owned_by(Some(Availability::Available(TokenOwner(user_id(
        "lichess4545",
    )))));
    harness.await_requests(4).await;
    assert_eq!(harness.lichess.connection_sizes(), [1, 401, 500, 900]);

    harness.owned_by(Some(Availability::Unavailable(Unavailable {
        recovery: Recovery::Recoverable,
        error: "answered 503 Service Unavailable".to_owned(),
    })));
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(harness.lichess.request_count(), 4);
}

#[sqlx::test]
async fn a_rate_limited_connect_is_retried_only_after_the_rate_limit_backoff(pool: PgPool) {
    let rate_limit_backoff = Duration::from_millis(300);
    let harness = harness(
        pool,
        lichess::StreamConfig {
            rate_limit_backoff,
            ..config()
        },
    )
    .await;
    harness
        .lichess
        .answer_next_with(Answer::Status(StatusCode::TOO_MANY_REQUESTS));
    harness.watch(&["alice", "bob"]).await;
    harness.await_requests(2).await;
    assert!(harness.lichess.gap_before(1) >= rate_limit_backoff);
}

#[sqlx::test]
async fn a_heartbeat_resets_the_reconnect_backoff(pool: PgPool) {
    let initial_backoff = Duration::from_millis(500);
    let harness = harness(
        pool,
        lichess::StreamConfig {
            initial_backoff,
            max_backoff: Duration::from_secs(10),
            ..config()
        },
    )
    .await;
    harness
        .lichess
        .answer_next_with(Answer::Status(StatusCode::BAD_GATEWAY));
    harness.lichess.answer_next_with(Answer::Lines(&[""]));
    harness.watch(&["alice", "bob"]).await;
    harness.await_requests(3).await;
    let after_the_heartbeat = harness.lichess.gap_before(2);
    assert!(
        after_the_heartbeat < initial_backoff * 3 / 2,
        "waited {after_the_heartbeat:?} after a heartbeat"
    );
}

#[sqlx::test]
async fn connections_report_up_to_300_players_without_a_token(pool: PgPool) {
    let mut harness = harness(pool, config()).await;
    let names = player_names(301);
    harness.watch(&names).await;
    let mut reported = [
        harness.next_connection().await.players,
        harness.next_connection().await.players,
    ];
    reported.sort_unstable();
    assert_eq!(reported, [1, 300]);
}

#[sqlx::test]
async fn a_failed_connect_is_not_reported_as_connected(pool: PgPool) {
    let mut harness = harness(pool, config()).await;
    harness
        .lichess
        .answer_next_with(Answer::Status(StatusCode::BAD_GATEWAY));
    harness.watch(&["alice", "bob"]).await;
    assert_eq!(
        harness.next_connection().await,
        StreamConnected { players: 2 }
    );
    assert_eq!(harness.lichess.request_count(), 2);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(harness.connections.try_recv().is_err());
}
