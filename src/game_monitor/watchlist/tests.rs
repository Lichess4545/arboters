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
use crate::heltour::team_pairing::{Board, BoardNumber, BoardResult, HalfPoints};
use crate::heltour::{HeltourSettings, League, Pairing, TeamPairing, league, team};
use crate::lichess::{self, Clock, Variant};

const PATIENCE: Duration = Duration::from_secs(5);
const LEAGUES: &str = r#"{"leagues": [{"tag": "classical", "name": "Classical League", "time_control": {"initial": 2700, "increment": 45}, "variant": "standard", "rated": true}, {"tag": "rapid", "name": "Rapid Battle", "time_control": null, "variant": "chess960", "rated": false}]}"#;
const ALICE_AND_BOB: &str = r#"{"pairings": [{"league": "classical", "season": "47", "round": 3, "white": "Alice", "black": "bob", "game_link": "", "result": ""}]}"#;
const ALICE_BOB_AND_CAROL_DAVE: &str = r#"{"pairings": [{"league": "classical", "season": "47", "round": 3, "white": "alice", "black": "bob", "game_link": "", "result": ""}, {"league": "rapid", "season": "12", "round": 1, "white": "Carol", "black": "dave", "game_link": "", "result": ""}]}"#;
const NO_ACTIVE_ROUND: &str = r#"{"pairings": null, "error": "no_matching_rounds"}"#;
const NO_TEAM_PAIRINGS: &str = r#"{"team_pairings": []}"#;
const KNIGHTS_AND_ROOKS: &str = r#"{"team_pairings": [{"league": "classical", "season": "47", "round": 3, "white_team": "Knights", "white_team_number": 1, "white_points": 1.5, "black_team": "Rooks", "black_team_number": 2, "black_points": 0.5, "boards": [{"board": 1, "white": "Alice", "black": "bob", "white_team": "Knights", "black_team": "Rooks", "game_link": "https://lichess.org/abcd1234", "result": "1/2-1/2"}, {"board": 2, "white": null, "black": "carol", "white_team": "Rooks", "black_team": "Knights", "game_link": "", "result": "0F-1X"}, {"board": 3, "white": "dave", "black": "erin", "white_team": "Knights", "black_team": "Rooks", "game_link": "", "result": ""}]}, {"league": "classical", "season": "47", "round": 3, "white_team": "Bishops", "white_team_number": 3, "white_points": 0.0, "black_team": "Pawns", "black_team_number": 4, "black_points": 0.0, "boards": []}]}"#;

struct Request {
    path: String,
    query: Option<String>,
    headers: HeaderMap,
}

#[derive(Clone)]
struct FakeHeltour {
    requests: Arc<Mutex<Vec<Request>>>,
    pairings: Arc<Mutex<VecDeque<(StatusCode, &'static str)>>>,
    team_pairings: (StatusCode, &'static str),
}

impl FakeHeltour {
    fn answer(&self, request: Request) -> (StatusCode, &'static str) {
        let path = request.path.clone();
        self.requests.lock().unwrap().push(request);
        if path.ends_with("/get_leagues/") {
            return (StatusCode::OK, LEAGUES);
        }
        if path.ends_with("/get_team_pairings/") {
            return self.team_pairings;
        }
        let mut pairings = self.pairings.lock().unwrap();
        if pairings.len() > 1 {
            pairings.pop_front().unwrap()
        } else {
            pairings[0]
        }
    }

    fn pairing_request_count(&self) -> usize {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|request| request.path.ends_with("/find_pairing/"))
            .count()
    }
}

async fn heltour_answering(
    pairings: &[(StatusCode, &'static str)],
    team_pairings: (StatusCode, &'static str),
) -> (FakeHeltour, Heltour) {
    let fake = FakeHeltour {
        requests: Arc::default(),
        pairings: Arc::new(Mutex::new(pairings.iter().copied().collect())),
        team_pairings,
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
        .route("/api/get_leagues/", answer.clone())
        .route("/api/get_team_pairings/", answer);
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
        Self::start_with_team_pairings(
            pairings,
            (StatusCode::OK, NO_TEAM_PAIRINGS),
            refresh_interval,
        )
        .await
    }

    async fn start_with_team_pairings(
        pairings: &[(StatusCode, &'static str)],
        team_pairings: (StatusCode, &'static str),
        refresh_interval: Duration,
    ) -> Self {
        let (heltour, client) = heltour_answering(pairings, team_pairings).await;
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
                        name: league::Name::try_new("Classical League").unwrap(),
                        time_control: Some(Clock {
                            initial_seconds: 2700,
                            increment_seconds: 45,
                        }),
                        variant: Variant::Standard,
                        rated: true,
                    },
                    League {
                        tag: tag("rapid"),
                        name: league::Name::try_new("Rapid Battle").unwrap(),
                        time_control: None,
                        variant: Variant::Chess960,
                        rated: false,
                    },
                ],
                pairings: vec![
                    pairing("classical", "alice", "bob"),
                    pairing("rapid", "carol", "dave"),
                ],
                team_pairings: Vec::new(),
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
    assert_eq!(
        paths,
        [
            "/api/find_pairing/",
            "/api/get_leagues/",
            "/api/get_team_pairings/"
        ]
    );
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
        syncing.heltour.pairing_request_count() >= 3
    })
    .await;
    assert!(syncing.told.try_recv().is_err());
    assert!(!syncing.sync.is_finished());
}

#[tokio::test]
async fn sync_ends_once_a_follower_has_stopped() {
    let (_, heltour) = heltour_answering(
        &[(StatusCode::OK, ALICE_AND_BOB)],
        (StatusCode::OK, NO_TEAM_PAIRINGS),
    )
    .await;
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

#[tokio::test]
async fn followers_are_sent_the_team_pairings_with_their_points_and_boards() {
    let mut syncing = Syncing::start_with_team_pairings(
        &[(StatusCode::OK, ALICE_AND_BOB)],
        (StatusCode::OK, KNIGHTS_AND_ROOKS),
        Duration::from_hours(1),
    )
    .await;

    let team = |name: &str| team::DisplayName::try_new(name).unwrap();
    let board = |number, white: Option<&str>, black, white_team, black_team| Board {
        number: BoardNumber::new(number),
        white: white.map(user_id),
        black: Some(user_id(black)),
        white_team: team(white_team),
        black_team: team(black_team),
        game_link: None,
        result: None,
    };
    assert_eq!(
        syncing.next().await.1.team_pairings,
        [
            TeamPairing {
                league: tag("classical"),
                white_team: team("Knights"),
                white_points: HalfPoints::new(3),
                black_team: team("Rooks"),
                black_points: HalfPoints::new(1),
                boards: vec![
                    Board {
                        game_link: Some(Url::parse("https://lichess.org/abcd1234").unwrap()),
                        result: Some(BoardResult::Draw),
                        ..board(1, Some("alice"), "bob", "Knights", "Rooks")
                    },
                    Board {
                        result: Some(BoardResult::BlackWonByForfeit),
                        ..board(2, None, "carol", "Rooks", "Knights")
                    },
                    board(3, Some("dave"), "erin", "Knights", "Rooks"),
                ],
            },
            TeamPairing {
                league: tag("classical"),
                white_team: team("Bishops"),
                white_points: HalfPoints::ZERO,
                black_team: team("Pawns"),
                black_points: HalfPoints::ZERO,
                boards: Vec::new(),
            },
        ]
    );
}

#[tokio::test]
async fn without_the_team_pairings_followers_are_still_sent_the_leagues_and_pairings() {
    let mut syncing = Syncing::start_with_team_pairings(
        &[(StatusCode::OK, ALICE_AND_BOB)],
        (StatusCode::NOT_FOUND, "Not Found"),
        Duration::from_hours(1),
    )
    .await;

    let (watch, active) = syncing.next().await;
    assert_eq!(watch, watching(&["alice", "bob"]));
    assert_eq!(active.leagues.len(), 2);
    assert_eq!(
        active.pairings,
        [Pairing {
            league: tag("classical"),
            white: user_id("alice"),
            black: user_id("bob"),
        }]
    );
    assert_eq!(active.team_pairings, []);
}
