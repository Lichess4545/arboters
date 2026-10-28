use std::collections::VecDeque;
use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::Router;
use axum::extract::RawQuery;
use axum::http::header::USER_AGENT;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::get;
use kameo::Actor;
use kameo::actor::{ActorRef, Spawn};
use kameo::message::{Context, Message};
use tokio::net::TcpListener;
use tokio::sync::mpsc;

use super::*;
use crate::game_monitor::http_client;
use crate::lichess;

const PATIENCE: Duration = Duration::from_secs(5);
const ALICE_AND_BOB: &str = r#"{"pairings": [{"league": "team4545", "season": "47", "round": 3, "white_team": "Knights", "white_team_number": 1, "black_team": "Rooks", "black_team_number": 2, "white": "Alice", "white_rating": 1850, "black": "bob", "black_rating": 1790, "game_link": "", "result": "", "datetime": null}]}"#;
const ALICE_BOB_AND_CAROL_DAVE: &str = r#"{"pairings": [{"league": "team4545", "season": "47", "round": 3, "white": "alice", "white_rating": 1850, "black": "bob", "black_rating": 1790, "game_link": "", "result": "", "datetime": null}, {"league": "team4545", "season": "47", "round": 3, "white": "Carol", "white_rating": null, "black": "dave", "black_rating": null, "game_link": "https://lichess.org/abcd1234", "result": "1-0", "datetime": "2026-10-28T19:00:00Z"}]}"#;
const NO_ACTIVE_ROUND: &str = r#"{"pairings": null, "error": "no_matching_rounds"}"#;

struct Request {
    query: Option<String>,
    headers: HeaderMap,
}

#[derive(Clone, Default)]
struct FakeHeltour {
    requests: Arc<Mutex<Vec<Request>>>,
    answers: Arc<Mutex<VecDeque<(StatusCode, &'static str)>>>,
}

impl FakeHeltour {
    fn answer(&self, query: Option<String>, headers: HeaderMap) -> (StatusCode, &'static str) {
        self.requests
            .lock()
            .unwrap()
            .push(Request { query, headers });
        let mut answers = self.answers.lock().unwrap();
        if answers.len() > 1 {
            answers.pop_front().unwrap()
        } else {
            answers[0]
        }
    }

    fn request_count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }
}

async fn heltour_answering(answers: &[(StatusCode, &'static str)]) -> (FakeHeltour, Heltour) {
    let fake = FakeHeltour {
        answers: Arc::new(Mutex::new(answers.iter().copied().collect())),
        ..FakeHeltour::default()
    };
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let answering = fake.clone();
    let router = Router::new().route(
        "/api/find_pairing/",
        get(move |RawQuery(query): RawQuery, headers: HeaderMap| {
            std::future::ready(answering.answer(query, headers))
        }),
    );
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let settings = HeltourSettings {
        base_url: Url::parse(&format!("http://{address}/api/")).unwrap(),
        token: Some(SecretString::from("heltour_secret")),
    };
    (
        fake,
        Heltour::new(http_client().unwrap(), &settings).unwrap(),
    )
}

struct WatchRecorder(mpsc::UnboundedSender<Watch>);

impl Actor for WatchRecorder {
    type Args = Self;
    type Error = Infallible;

    fn on_start(
        recorder: Self,
        _: ActorRef<Self>,
    ) -> impl Future<Output = Result<Self, Self::Error>> + Send {
        std::future::ready(Ok(recorder))
    }
}

impl Message<Watch> for WatchRecorder {
    type Reply = ();

    async fn handle(&mut self, watch: Watch, _: &mut Context<Self, Self::Reply>) {
        self.0.send(watch).unwrap();
    }
}

fn recording_monitor() -> (ActorRef<WatchRecorder>, mpsc::UnboundedReceiver<Watch>) {
    let (sender, watches) = mpsc::unbounded_channel();
    (WatchRecorder::spawn(WatchRecorder(sender)), watches)
}

async fn next_watch(watches: &mut mpsc::UnboundedReceiver<Watch>) -> Watch {
    tokio::time::timeout(PATIENCE, watches.recv())
        .await
        .unwrap()
        .unwrap()
}

fn team4545() -> league::Tag {
    league::Tag::try_new("team4545").unwrap()
}

fn watching(names: &[&str]) -> Watch {
    Watch(
        names
            .iter()
            .map(|name| lichess::UserId::try_new(*name).unwrap())
            .collect(),
    )
}

async fn eventually(what: &str, condition: impl Fn() -> bool) {
    let deadline = Instant::now() + PATIENCE;
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting until {what}");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

#[tokio::test]
async fn the_monitor_is_told_both_players_of_every_current_pairing() {
    let (_, heltour) = heltour_answering(&[(StatusCode::OK, ALICE_BOB_AND_CAROL_DAVE)]).await;
    let (monitor, mut watches) = recording_monitor();
    let sync = tokio::spawn(keep_in_sync(
        heltour,
        team4545(),
        monitor.recipient(),
        Duration::from_hours(1),
    ));

    assert_eq!(
        next_watch(&mut watches).await,
        watching(&["alice", "bob", "carol", "dave"])
    );
    sync.abort();
}

#[tokio::test]
async fn pairings_are_asked_for_the_league_with_the_api_token_and_our_user_agent() {
    let (fake, heltour) = heltour_answering(&[(StatusCode::OK, ALICE_AND_BOB)]).await;
    let (monitor, mut watches) = recording_monitor();
    let sync = tokio::spawn(keep_in_sync(
        heltour,
        team4545(),
        monitor.recipient(),
        Duration::from_hours(1),
    ));
    next_watch(&mut watches).await;
    sync.abort();

    let Request { query, headers } = fake.requests.lock().unwrap().remove(0);
    assert_eq!(query.as_deref(), Some("league=team4545"));
    assert_eq!(headers[AUTHORIZATION], "Token heltour_secret");
    assert_eq!(
        headers[USER_AGENT],
        format!(
            "arboters/{} (+https://github.com/Lichess4545/arboters)",
            env!("CARGO_PKG_VERSION")
        )
        .as_str()
    );
}

#[tokio::test]
async fn without_an_active_round_no_one_is_watched() {
    let (_, heltour) = heltour_answering(&[(StatusCode::OK, NO_ACTIVE_ROUND)]).await;
    let (monitor, mut watches) = recording_monitor();
    let sync = tokio::spawn(keep_in_sync(
        heltour,
        team4545(),
        monitor.recipient(),
        Duration::from_hours(1),
    ));

    assert_eq!(next_watch(&mut watches).await, watching(&[]));
    sync.abort();
}

#[tokio::test]
async fn new_pairings_reach_the_monitor_at_a_later_refresh() {
    let (_, heltour) = heltour_answering(&[
        (StatusCode::OK, ALICE_AND_BOB),
        (StatusCode::OK, ALICE_BOB_AND_CAROL_DAVE),
    ])
    .await;
    let (monitor, mut watches) = recording_monitor();
    let sync = tokio::spawn(keep_in_sync(
        heltour,
        team4545(),
        monitor.recipient(),
        Duration::from_millis(50),
    ));

    assert_eq!(next_watch(&mut watches).await, watching(&["alice", "bob"]));
    assert_eq!(
        next_watch(&mut watches).await,
        watching(&["alice", "bob", "carol", "dave"])
    );
    sync.abort();
}

#[tokio::test]
async fn a_refused_request_keeps_the_previous_watchlist_and_sync_carries_on() {
    let (fake, heltour) = heltour_answering(&[
        (StatusCode::OK, ALICE_AND_BOB),
        (StatusCode::UNAUTHORIZED, "Unauthorized"),
    ])
    .await;
    let (monitor, mut watches) = recording_monitor();
    let sync = tokio::spawn(keep_in_sync(
        heltour,
        team4545(),
        monitor.recipient(),
        Duration::from_millis(50),
    ));

    assert_eq!(next_watch(&mut watches).await, watching(&["alice", "bob"]));
    eventually("heltour has refused twice", || fake.request_count() >= 3).await;
    assert!(watches.try_recv().is_err());
    assert!(!sync.is_finished());
    sync.abort();
}

#[tokio::test]
async fn sync_ends_once_the_monitor_has_stopped() {
    let (_, heltour) = heltour_answering(&[(StatusCode::OK, ALICE_AND_BOB)]).await;
    let (monitor, _watches) = recording_monitor();
    monitor.stop_gracefully().await.unwrap();
    monitor.wait_for_shutdown().await;

    let sync = keep_in_sync(
        heltour,
        team4545(),
        monitor.recipient(),
        Duration::from_hours(1),
    );

    assert!(tokio::time::timeout(PATIENCE, sync).await.is_ok());
}
