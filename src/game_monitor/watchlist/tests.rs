use std::collections::VecDeque;
use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::Router;
use axum::extract::{OriginalUri, RawQuery};
use axum::http::header::{AUTHORIZATION, USER_AGENT};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::get;
use kameo::Actor;
use kameo::actor::{ActorRef, Spawn};
use kameo::message::{Context, Message};
use secrecy::SecretString;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use url::Url;

use super::*;
use crate::game_monitor::http_client;
use crate::heltour::{HeltourSettings, League, Pairing, league};
use crate::lichess::{self, Clock, Variant};

const PATIENCE: Duration = Duration::from_secs(5);
const LEAGUES: &str = r#"{"leagues": [{"tag": "classical", "name": "Classical League", "time_control": {"initial": 2700, "increment": 45}, "variant": "standard", "rated": true}, {"tag": "rapid", "name": "Rapid Battle", "time_control": null, "variant": "chess960", "rated": false}]}"#;
const ALICE_AND_BOB: &str = r#"{"pairings": [{"league": "classical", "season": "47", "round": 3, "white": "Alice", "black": "bob", "game_link": "", "result": ""}]}"#;
const ALICE_BOB_AND_CAROL_DAVE: &str = r#"{"pairings": [{"league": "classical", "season": "47", "round": 3, "white": "alice", "black": "bob", "game_link": "", "result": ""}, {"league": "rapid", "season": "12", "round": 1, "white": "Carol", "black": "dave", "game_link": "", "result": ""}]}"#;
const NO_ACTIVE_ROUND: &str = r#"{"pairings": null, "error": "no_matching_rounds"}"#;

struct Request {
    path: String,
    query: Option<String>,
    headers: HeaderMap,
}

#[derive(Clone, Default)]
struct FakeHeltour {
    requests: Arc<Mutex<Vec<Request>>>,
    pairings: Arc<Mutex<VecDeque<(StatusCode, &'static str)>>>,
}

impl FakeHeltour {
    fn answer(&self, request: Request) -> (StatusCode, &'static str) {
        let leagues = request.path.ends_with("/get_leagues/");
        self.requests.lock().unwrap().push(request);
        if leagues {
            return (StatusCode::OK, LEAGUES);
        }
        let mut pairings = self.pairings.lock().unwrap();
        if pairings.len() > 1 {
            pairings.pop_front().unwrap()
        } else {
            pairings[0]
        }
    }

    fn request_count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }
}

async fn heltour_answering(pairings: &[(StatusCode, &'static str)]) -> (FakeHeltour, Heltour) {
    let fake = FakeHeltour {
        pairings: Arc::new(Mutex::new(pairings.iter().copied().collect())),
        ..FakeHeltour::default()
    };
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let answering = fake.clone();
    let answer = get(
        move |OriginalUri(uri): OriginalUri, RawQuery(query): RawQuery, headers: HeaderMap| {
            std::future::ready(answering.answer(Request {
                path: uri.path().to_owned(),
                query,
                headers,
            }))
        },
    );
    let router = Router::new()
        .route("/api/find_pairing/", answer.clone())
        .route("/api/get_leagues/", answer);
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

#[derive(Debug)]
enum Told {
    Watch(Watch),
    Active(ActiveLeagues),
}

struct Recorder(mpsc::UnboundedSender<Told>);

impl Actor for Recorder {
    type Args = Self;
    type Error = Infallible;

    fn on_start(
        recorder: Self,
        _: ActorRef<Self>,
    ) -> impl Future<Output = Result<Self, Self::Error>> + Send {
        std::future::ready(Ok(recorder))
    }
}

impl Message<Watch> for Recorder {
    type Reply = ();

    async fn handle(&mut self, watch: Watch, _: &mut Context<Self, Self::Reply>) {
        self.0.send(Told::Watch(watch)).unwrap();
    }
}

impl Message<ActiveLeagues> for Recorder {
    type Reply = ();

    async fn handle(&mut self, active: ActiveLeagues, _: &mut Context<Self, Self::Reply>) {
        self.0.send(Told::Active(active)).unwrap();
    }
}

struct Syncing {
    heltour: FakeHeltour,
    sync: tokio::task::JoinHandle<()>,
    told: mpsc::UnboundedReceiver<Told>,
}

impl Syncing {
    async fn start(pairings: &[(StatusCode, &'static str)], refresh_interval: Duration) -> Self {
        let (heltour, client) = heltour_answering(pairings).await;
        let (sender, told) = mpsc::unbounded_channel();
        let recorder = Recorder::spawn(Recorder(sender));
        let sync = tokio::spawn(keep_in_sync(
            client,
            recorder.clone().recipient(),
            vec![recorder.recipient()],
            refresh_interval,
        ));
        Self {
            heltour,
            sync,
            told,
        }
    }

    async fn next(&mut self) -> (Watch, ActiveLeagues) {
        let mut next = async || {
            tokio::time::timeout(PATIENCE, self.told.recv())
                .await
                .unwrap()
                .unwrap()
        };
        match (next().await, next().await) {
            (Told::Watch(watch), Told::Active(active)) => (watch, active),
            told => panic!("expected a watch and then the active leagues, got {told:?}"),
        }
    }

    async fn next_watch(&mut self) -> Watch {
        self.next().await.0
    }
}

impl Drop for Syncing {
    fn drop(&mut self) {
        self.sync.abort();
    }
}

fn user_id(name: &str) -> lichess::UserId {
    lichess::UserId::try_new(name).unwrap()
}

fn tag(tag: &str) -> league::Tag {
    league::Tag::try_new(tag).unwrap()
}

fn watching(names: &[&str]) -> Watch {
    Watch(names.iter().map(|name| user_id(name)).collect())
}

async fn eventually(what: &str, condition: impl Fn() -> bool) {
    let deadline = Instant::now() + PATIENCE;
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting until {what}");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

#[tokio::test]
async fn the_monitor_is_told_every_paired_player_and_followers_the_active_leagues() {
    let mut syncing = Syncing::start(
        &[(StatusCode::OK, ALICE_BOB_AND_CAROL_DAVE)],
        Duration::from_hours(1),
    )
    .await;

    let pairing = |league: &str, white: &str, black: &str| Pairing {
        league: tag(league),
        white: user_id(white),
        black: user_id(black),
    };
    assert_eq!(
        syncing.next().await,
        (
            watching(&["alice", "bob", "carol", "dave"]),
            ActiveLeagues {
                leagues: vec![
                    League {
                        tag: tag("classical"),
                        time_control: Some(Clock {
                            initial_seconds: 2700,
                            increment_seconds: 45,
                        }),
                        variant: Variant::Standard,
                        rated: true,
                    },
                    League {
                        tag: tag("rapid"),
                        time_control: None,
                        variant: Variant::Chess960,
                        rated: false,
                    },
                ],
                pairings: vec![
                    pairing("classical", "alice", "bob"),
                    pairing("rapid", "carol", "dave"),
                ],
            }
        )
    );
}

#[tokio::test]
async fn heltour_is_asked_about_every_league_with_the_api_token_and_our_user_agent() {
    let mut syncing =
        Syncing::start(&[(StatusCode::OK, ALICE_AND_BOB)], Duration::from_hours(1)).await;
    syncing.next_watch().await;

    let mut requests = std::mem::take(&mut *syncing.heltour.requests.lock().unwrap());
    requests.sort_by(|left, right| left.path.cmp(&right.path));
    let paths: Vec<_> = requests
        .iter()
        .map(|request| request.path.as_str())
        .collect();
    assert_eq!(paths, ["/api/find_pairing/", "/api/get_leagues/"]);
    for request in requests {
        assert_eq!(request.query, None);
        assert_eq!(request.headers[AUTHORIZATION], "Token heltour_secret");
        assert_eq!(
            request.headers[USER_AGENT],
            format!(
                "arboters/{} (+https://github.com/Lichess4545/arboters)",
                env!("CARGO_PKG_VERSION")
            )
            .as_str()
        );
    }
}

#[tokio::test]
async fn without_an_active_round_no_one_is_watched() {
    let mut syncing = Syncing::start(
        &[(StatusCode::OK, NO_ACTIVE_ROUND)],
        Duration::from_hours(1),
    )
    .await;
    assert_eq!(syncing.next_watch().await, watching(&[]));
}

#[tokio::test]
async fn new_pairings_reach_the_monitor_at_a_later_refresh() {
    let mut syncing = Syncing::start(
        &[
            (StatusCode::OK, ALICE_AND_BOB),
            (StatusCode::OK, ALICE_BOB_AND_CAROL_DAVE),
        ],
        Duration::from_millis(50),
    )
    .await;

    assert_eq!(syncing.next_watch().await, watching(&["alice", "bob"]));
    assert_eq!(
        syncing.next_watch().await,
        watching(&["alice", "bob", "carol", "dave"])
    );
}

#[tokio::test]
async fn a_refused_request_keeps_the_previous_watchlist_and_sync_carries_on() {
    let mut syncing = Syncing::start(
        &[
            (StatusCode::OK, ALICE_AND_BOB),
            (StatusCode::UNAUTHORIZED, "Unauthorized"),
        ],
        Duration::from_millis(50),
    )
    .await;

    assert_eq!(syncing.next_watch().await, watching(&["alice", "bob"]));
    eventually("heltour has refused twice", || {
        syncing.heltour.request_count() >= 6
    })
    .await;
    assert!(syncing.told.try_recv().is_err());
    assert!(!syncing.sync.is_finished());
}

#[tokio::test]
async fn sync_ends_once_a_follower_has_stopped() {
    let (_, heltour) = heltour_answering(&[(StatusCode::OK, ALICE_AND_BOB)]).await;
    let (sender, _told) = mpsc::unbounded_channel();
    let monitor = Recorder::spawn(Recorder(sender.clone()));
    let stopped = Recorder::spawn(Recorder(sender));
    stopped.stop_gracefully().await.unwrap();
    stopped.wait_for_shutdown().await;

    let sync = keep_in_sync(
        heltour,
        monitor.recipient(),
        vec![stopped.recipient()],
        Duration::from_hours(1),
    );

    assert!(tokio::time::timeout(PATIENCE, sync).await.is_ok());
}
