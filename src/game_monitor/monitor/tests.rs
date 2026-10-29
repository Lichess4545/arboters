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
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use url::Url;

use super::*;
use crate::game_monitor::{GameOutcome, LichessSettings, StreamConnected, http_client};
use crate::lichess::{Game, GameId, Player, Speed};

const STARTED: &str = r#"{"id":"g1","rated":true,"speed":"classical","status":20,"players":{"white":{"userId":"alice"},"black":{"userId":"bob"}}}"#;
const WHITE_WON: &str = r#"{"id":"g1","rated":true,"speed":"classical","status":30,"winner":"white","players":{"white":{"userId":"alice"},"black":{"userId":"bob"}}}"#;

enum Answer {
    Status(StatusCode),
    Lines(&'static [&'static str]),
}

struct Request {
    user_ids: Vec<lichess::UserId>,
    at: Instant,
    lines: Option<UnboundedSender<Result<String, Infallible>>>,
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
        let user_ids = body
            .split(',')
            .map(|name| lichess::UserId::try_new(name).unwrap())
            .collect();
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

struct Collector<T>(mpsc::UnboundedSender<T>);

impl<T: Send + 'static> Actor for Collector<T> {
    type Args = Self;
    type Error = Infallible;

    fn on_start(
        collector: Self,
        _: ActorRef<Self>,
    ) -> impl Future<Output = Result<Self, Self::Error>> + Send {
        std::future::ready(Ok(collector))
    }
}

impl<T: Send + 'static> Message<T> for Collector<T> {
    type Reply = ();

    async fn handle(&mut self, collected: T, _: &mut Context<Self, Self::Reply>) {
        self.0.send(collected).unwrap();
    }
}

struct Harness {
    lichess: FakeLichess,
    monitor: ActorRef<GameMonitor>,
    events: mpsc::UnboundedReceiver<GameEvent>,
    connections: mpsc::UnboundedReceiver<StreamConnected>,
    token: watch::Sender<TokenState>,
}

impl Harness {
    async fn watch(&self, names: &[impl AsRef<str> + Sync]) {
        let names: Vec<&str> = names.iter().map(AsRef::as_ref).collect();
        let watch = Watch(user_ids(&names).into_iter().collect());
        self.monitor.tell(watch).await.unwrap();
    }

    async fn next_event(&mut self) -> GameEvent {
        next(&mut self.events).await
    }

    async fn follower(
        &self,
    ) -> (
        ActorRef<Collector<GameEvent>>,
        mpsc::UnboundedReceiver<GameEvent>,
    ) {
        let (sender, events) = mpsc::unbounded_channel();
        let follower = Collector::spawn(Collector(sender));
        self.monitor
            .tell(Follow(follower.clone().recipient()))
            .await
            .unwrap();
        (follower, events)
    }

    async fn watching_alice_and_bob(&self) {
        self.watch(&["alice", "bob"]).await;
        eventually("the game is watched", || self.lichess.request_count() == 1).await;
    }

    async fn next_connection(&mut self) -> StreamConnected {
        tokio::time::timeout(Duration::from_secs(5), self.connections.recv())
            .await
            .unwrap()
            .unwrap()
    }
}

async fn next(events: &mut mpsc::UnboundedReceiver<GameEvent>) -> GameEvent {
    tokio::time::timeout(Duration::from_secs(5), events.recv())
        .await
        .unwrap()
        .unwrap()
}

async fn harness(stream: StreamSettings) -> Harness {
    harness_with_token(None, stream).await
}

async fn harness_with_token(token: Option<&str>, stream: StreamSettings) -> Harness {
    let lichess = FakeLichess::default();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let answering = lichess.clone();
    let router = Router::new().route(
        "/api/stream/games-by-users",
        post(move |body: String| std::future::ready(answering.answer(&body))),
    );
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let settings = LichessSettings {
        base_url: Url::parse(&format!("http://{address}")).unwrap(),
        token: token.map(SecretString::from),
        stream,
        ..LichessSettings::default()
    };
    let (event_sender, events) = mpsc::unbounded_channel();
    let (connection_sender, connections) = mpsc::unbounded_channel();
    let (token_sender, token) = watch::channel(TokenState::Untested);
    let monitor = GameMonitor::spawn(GameMonitor::new(
        LichessStream::new(http_client().unwrap(), &settings).unwrap(),
        settings.stream,
        Collector::spawn(Collector(connection_sender)).recipient(),
        token,
    ));
    monitor
        .tell(Follow(
            Collector::spawn(Collector(event_sender)).recipient(),
        ))
        .await
        .unwrap();
    Harness {
        lichess,
        monitor,
        events,
        connections,
        token: token_sender,
    }
}

fn settings() -> StreamSettings {
    StreamSettings {
        initial_backoff: Duration::from_millis(10),
        max_backoff: Duration::from_secs(1),
        rate_limit_backoff: Duration::from_millis(10),
        ..StreamSettings::default()
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

fn user_ids(names: &[&str]) -> Vec<lichess::UserId> {
    names
        .iter()
        .map(|name| lichess::UserId::try_new(*name).unwrap())
        .collect()
}

fn game() -> Game {
    let player = |name: &str| Player {
        user_id: lichess::UserId::try_new(name).unwrap(),
        rating: None,
    };
    Game {
        id: GameId::new("g1"),
        white: player("alice"),
        black: player("bob"),
        speed: Speed::Classical,
        rated: true,
    }
}

#[tokio::test]
async fn without_a_token_each_connection_watches_up_to_300_users() {
    let harness = harness(settings()).await;
    let names = player_names(301);
    harness.watch(&names).await;
    eventually("both connections open", || {
        harness.lichess.request_count() == 2
    })
    .await;
    assert_eq!(harness.lichess.connection_sizes(), [1, 300]);
}

#[tokio::test]
async fn with_a_token_each_connection_watches_up_to_500_users() {
    let harness = harness_with_token(Some("lip_secret"), settings()).await;
    let names = player_names(501);
    harness.watch(&names).await;
    eventually("both connections open", || {
        harness.lichess.request_count() == 2
    })
    .await;
    assert_eq!(harness.lichess.connection_sizes(), [1, 500]);
}

#[tokio::test]
async fn the_next_refresh_after_the_token_owner_is_known_splits_the_watchlist_again() {
    let harness = harness_with_token(Some("lip_secret"), settings()).await;
    let names = player_names(901);
    harness.watch(&names).await;
    eventually("two connections open", || {
        harness.lichess.request_count() == 2
    })
    .await;
    harness.token.send_replace(TokenState::Valid {
        owner: lichess::UserId::try_new("lichess4545").unwrap(),
    });
    harness.watch(&names).await;
    eventually("the watchlist is split again", || {
        harness.lichess.request_count() == 4
    })
    .await;
    assert_eq!(harness.lichess.connection_sizes(), [1, 401, 500, 900]);
}

#[tokio::test]
async fn an_unchanged_watchlist_does_not_reconnect() {
    let harness = harness(settings()).await;
    harness.watch(&["alice"]).await;
    eventually("alice is watched", || harness.lichess.request_count() == 1).await;
    harness.watch(&["alice"]).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(harness.lichess.request_count(), 1);
}

#[tokio::test]
async fn a_changed_watchlist_closes_the_old_connection_and_opens_a_new_one() {
    let harness = harness(settings()).await;
    harness.watch(&["alice"]).await;
    eventually("alice is watched", || harness.lichess.request_count() == 1).await;
    harness.watch(&["alice", "bob"]).await;
    eventually("bob is watched", || harness.lichess.request_count() == 2).await;
    eventually("the old connection closes", || {
        !harness.lichess.stream_line(0, "")
    })
    .await;
    assert_eq!(
        harness.lichess.connected_user_ids(),
        [user_ids(&["alice"]), user_ids(&["alice", "bob"])]
    );
}

#[tokio::test]
async fn every_follower_receives_every_game_event() {
    let mut harness = harness(settings()).await;
    let (_second, mut second_events) = harness.follower().await;
    harness.watching_alice_and_bob().await;
    harness.lichess.stream_line(0, STARTED);
    harness.lichess.stream_line(0, WHITE_WON);
    let finished = GameEvent::Finished {
        game: game(),
        outcome: GameOutcome::WhiteWon,
    };
    assert_eq!(
        [harness.next_event().await, harness.next_event().await],
        [GameEvent::Started(game()), finished.clone()]
    );
    assert_eq!(
        [
            next(&mut second_events).await,
            next(&mut second_events).await
        ],
        [GameEvent::Started(game()), finished]
    );
}

#[tokio::test]
async fn a_stopped_follower_is_dropped_and_the_others_still_receive_events() {
    let harness = harness(settings()).await;
    let (stopped, _) = harness.follower().await;
    let (_still_following, mut events) = harness.follower().await;
    stopped.stop_gracefully().await.unwrap();
    stopped.wait_for_shutdown().await;
    harness.watching_alice_and_bob().await;
    harness.lichess.stream_line(0, STARTED);
    harness.lichess.stream_line(0, WHITE_WON);
    let finished = GameEvent::Finished {
        game: game(),
        outcome: GameOutcome::WhiteWon,
    };
    assert_eq!(
        [next(&mut events).await, next(&mut events).await],
        [GameEvent::Started(game()), finished]
    );
}

#[tokio::test]
async fn a_rate_limited_connect_is_retried_only_after_the_rate_limit_backoff() {
    let rate_limit_backoff = Duration::from_millis(300);
    let harness = harness(StreamSettings {
        rate_limit_backoff,
        ..settings()
    })
    .await;
    harness
        .lichess
        .answer_next_with(Answer::Status(StatusCode::TOO_MANY_REQUESTS));
    harness.watch(&["alice"]).await;
    eventually("the connect is retried", || {
        harness.lichess.request_count() == 2
    })
    .await;
    assert!(harness.lichess.gap_before(1) >= rate_limit_backoff);
}

#[tokio::test]
async fn an_ended_stream_is_reconnected() {
    let harness = harness(settings()).await;
    harness.lichess.answer_next_with(Answer::Lines(&[]));
    harness.watch(&["alice"]).await;
    eventually("the stream reconnects", || {
        harness.lichess.request_count() == 2
    })
    .await;
}

#[tokio::test]
async fn a_heartbeat_resets_the_reconnect_backoff() {
    let initial_backoff = Duration::from_millis(500);
    let harness = harness(StreamSettings {
        initial_backoff,
        max_backoff: Duration::from_secs(10),
        ..settings()
    })
    .await;
    harness
        .lichess
        .answer_next_with(Answer::Status(StatusCode::BAD_GATEWAY));
    harness.lichess.answer_next_with(Answer::Lines(&[""]));
    harness.watch(&["alice"]).await;
    eventually("the stream reconnects twice", || {
        harness.lichess.request_count() == 3
    })
    .await;
    let after_the_heartbeat = harness.lichess.gap_before(2);
    assert!(
        after_the_heartbeat < initial_backoff * 3 / 2,
        "waited {after_the_heartbeat:?} after a heartbeat"
    );
}

#[tokio::test]
async fn each_connection_reports_its_player_count_once_connected() {
    let mut harness = harness(settings()).await;
    let names = player_names(301);
    harness.watch(&names).await;
    let mut reported = [
        harness.next_connection().await.players,
        harness.next_connection().await.players,
    ];
    reported.sort_unstable();
    assert_eq!(reported, [1, 300]);
}

#[tokio::test]
async fn a_failed_connect_is_not_reported_as_connected() {
    let mut harness = harness(settings()).await;
    harness
        .lichess
        .answer_next_with(Answer::Status(StatusCode::BAD_GATEWAY));
    harness.watch(&["alice"]).await;
    assert_eq!(
        harness.next_connection().await,
        StreamConnected { players: 1 }
    );
    assert_eq!(harness.lichess.request_count(), 2);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(harness.connections.try_recv().is_err());
}
