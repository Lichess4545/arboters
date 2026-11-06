use std::convert::Infallible;

use kameo::Actor;
use kameo::actor::{ActorRef, Spawn};
use kameo::message::{Context, Message};
use reqwest::header::{AUTHORIZATION, USER_AGENT};
use secrecy::SecretString;
use serde_json::{Value, json};
use tokio::sync::{mpsc, watch};
use url::Url;
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockBuilder, MockServer, ResponseTemplate};

use super::*;
use crate::game_monitor::{FixturesId, http_client};
use crate::heltour::{self, League};
use crate::lichess::{Clock, Variant};
use crate::testing::{tag, user_id};

const PATIENCE: Duration = Duration::from_secs(5);
const ARBOTERS: &str = concat!(
    "arboters/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/Lichess4545/arboters)"
);
const ROUND_STARTS: i64 = 4_102_444_800_000;
const ROUND_ENDS: i64 = 4_103_049_600_000;
const ENDED: i64 = 1_000_000_000_000;

fn leagues() -> Value {
    json!({"leagues": [
        {"tag": "classical", "name": "Classical League", "timeControl": {"initial": 2700, "increment": 45}, "variant": "standard", "rated": true},
        {"tag": "rapid", "name": "Rapid Battle", "variant": "chess960", "rated": false},
    ]})
}

fn round(id: u32, starts: Option<i64>, ends: Option<i64>) -> Value {
    json!({"id": id, "startsAt": starts, "endsAt": ends})
}

fn current_round(id: u32) -> Value {
    round(id, Some(ROUND_STARTS), Some(ROUND_ENDS))
}

fn rounds(rounds: &[Value]) -> Value {
    json!({ "rounds": rounds })
}

fn pairing(white: Option<&str>, black: Option<&str>) -> Value {
    let side = |id: Option<&str>| json!({"user": id.map(|id| json!({ "id": id }))});
    json!({"players": {"white": side(white), "black": side(black)}})
}

fn game(white: &str, black: &str) -> Value {
    pairing(Some(white), Some(black))
}

fn pairings(pairings: &[Value]) -> Value {
    json!({ "pairings": pairings })
}

fn ok(body: Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(body)
}

fn asked_for(at: &str) -> MockBuilder {
    Mock::given(method("GET"))
        .and(path(at))
        .and(header(AUTHORIZATION, "Token heltour_secret"))
        .and(header(USER_AGENT, ARBOTERS))
}

fn asked_for_active(at: &str) -> MockBuilder {
    asked_for(at).and(query_param("active", "true"))
}

struct FakeHeltour(MockServer);

impl FakeHeltour {
    async fn with_leagues() -> Self {
        let heltour = Self(MockServer::start().await);
        heltour.leagues(&[ok(leagues())]).await;
        heltour
    }

    async fn answers(&self, request: impl Fn() -> MockBuilder, answers: &[ResponseTemplate]) {
        for answer in answers {
            request()
                .respond_with(answer.clone())
                .up_to_n_times(1)
                .mount(&self.0)
                .await;
        }
        if let Some(last) = answers.last() {
            request().respond_with(last.clone()).mount(&self.0).await;
        }
    }

    async fn leagues(&self, answers: &[ResponseTemplate]) {
        self.answers(|| asked_for_active("/api/v2/leagues/"), answers)
            .await;
    }

    async fn rounds_of(&self, league: &str, answers: &[ResponseTemplate]) {
        let at = format!("/api/v2/leagues/{league}/rounds/");
        self.answers(|| asked_for_active(&at), answers).await;
    }

    async fn pairings_of(&self, league: &str, round: u32, answers: &[ResponseTemplate]) {
        let at = format!("/api/v2/leagues/{league}/rounds/{round}/");
        self.answers(|| asked_for(&at), answers).await;
    }

    async fn playing(&self, league: &str, round: u32, games: &[Value]) {
        self.rounds_of(league, &[ok(rounds(&[current_round(round)]))])
            .await;
        self.pairings_of(league, round, &[ok(pairings(games))])
            .await;
    }

    fn client(&self) -> Heltour {
        let config = heltour::Config {
            base_url: Url::parse(&format!("{}/api/", self.0.uri())).unwrap(),
            token: SecretString::from("heltour_secret"),
        };
        Heltour::new(http_client().unwrap(), &config)
    }
}

async fn only_classical_playing(games: &[Value]) -> FakeHeltour {
    let heltour = FakeHeltour::with_leagues().await;
    heltour.playing("classical", 812, games).await;
    heltour.rounds_of("rapid", &[ok(rounds(&[]))]).await;
    heltour
}

struct Recorder(mpsc::UnboundedSender<NewFixtures>);

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

impl Message<SubmitFixtures> for Recorder {
    type Reply = ();

    async fn handle(
        &mut self,
        SubmitFixtures { fixtures, stored }: SubmitFixtures,
        _: &mut Context<Self, Self::Reply>,
    ) {
        self.0.send(fixtures).unwrap();
        stored.send(FixturesId::new(1)).unwrap();
    }
}

struct Syncing {
    heltour: FakeHeltour,
    sync: tokio::task::JoinHandle<()>,
    submitted: mpsc::UnboundedReceiver<NewFixtures>,
    active_leagues: watch::Receiver<ActiveLeagues>,
}

impl Syncing {
    fn start(heltour: FakeHeltour, refresh_interval: Duration) -> Self {
        let (sender, submitted) = mpsc::unbounded_channel();
        let (published, active_leagues) = watch::channel(ActiveLeagues::default());
        let sync = tokio::spawn(keep_in_sync(
            heltour.client(),
            Recorder::spawn(Recorder(sender)).recipient(),
            published,
            refresh_interval,
        ));
        Self {
            heltour,
            sync,
            submitted,
            active_leagues,
        }
    }

    async fn next_submitted(&mut self) -> NewFixtures {
        tokio::time::timeout(PATIENCE, self.submitted.recv())
            .await
            .unwrap()
            .unwrap()
    }

    async fn requests_reach(&self, count: usize) {
        let reached = async {
            while self.heltour.0.received_requests().await.unwrap().len() < count {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        };
        tokio::time::timeout(PATIENCE, reached).await.unwrap();
    }
}

impl Drop for Syncing {
    fn drop(&mut self) {
        self.sync.abort();
    }
}

fn at(epoch_millis: i64) -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(epoch_millis) * 1_000_000).unwrap()
}

fn classical() -> League {
    League {
        tag: tag("classical"),
        name: league::Name::try_new("Classical League").unwrap(),
        time_control: Some(Clock {
            initial_seconds: 2700,
            increment_seconds: 45,
        }),
        variant: Variant::Standard,
        rated: true,
    }
}

fn rapid() -> League {
    League {
        tag: tag("rapid"),
        name: league::Name::try_new("Rapid Battle").unwrap(),
        time_control: None,
        variant: Variant::Chess960,
        rated: false,
    }
}

fn fixtures(league: &League, games: &[(&str, &str)]) -> NewFixtures {
    NewFixtures {
        league: league.tag.monitored(),
        period: Period {
            starts: Some(at(ROUND_STARTS)),
            ends: Some(at(ROUND_ENDS)),
        },
        games: games
            .iter()
            .map(|(white, black)| Fixture {
                white: user_id(white),
                black: user_id(black),
                time_control: league.time_control,
                variant: league.variant,
                rated: league.rated,
            })
            .collect(),
    }
}

#[tokio::test]
async fn active_leagues_are_published_and_their_rounds_submitted() {
    let heltour = FakeHeltour::with_leagues().await;
    heltour
        .playing("classical", 812, &[game("alice", "bob")])
        .await;
    heltour
        .playing("rapid", 900, &[game("Carol", "dave")])
        .await;
    let mut syncing = Syncing::start(heltour, Duration::from_hours(1));

    assert_eq!(
        [
            syncing.next_submitted().await,
            syncing.next_submitted().await
        ],
        [
            fixtures(&classical(), &[("alice", "bob")]),
            fixtures(&rapid(), &[("carol", "dave")]),
        ]
    );
    assert_eq!(
        syncing.active_leagues.borrow().clone(),
        ActiveLeagues {
            leagues: vec![classical(), rapid()],
        }
    );
}

#[tokio::test]
async fn ended_rounds_are_left_out_of_the_fixtures() {
    let heltour = FakeHeltour::with_leagues().await;
    heltour
        .rounds_of("classical", &[ok(rounds(&[round(811, None, Some(ENDED))]))])
        .await;
    heltour
        .rounds_of(
            "rapid",
            &[ok(rounds(&[
                current_round(900),
                round(899, None, Some(ENDED)),
            ]))],
        )
        .await;
    heltour
        .pairings_of("rapid", 900, &[ok(pairings(&[game("carol", "dave")]))])
        .await;
    heltour
        .pairings_of("rapid", 899, &[ok(pairings(&[game("erin", "frank")]))])
        .await;
    let mut syncing = Syncing::start(heltour, Duration::from_hours(1));

    assert_eq!(
        syncing.next_submitted().await,
        fixtures(&rapid(), &[("carol", "dave")])
    );
}

#[tokio::test]
async fn new_pairings_are_submitted_at_a_later_refresh() {
    let heltour = FakeHeltour::with_leagues().await;
    heltour
        .rounds_of("classical", &[ok(rounds(&[current_round(812)]))])
        .await;
    heltour
        .pairings_of(
            "classical",
            812,
            &[
                ok(pairings(&[game("alice", "bob")])),
                ok(pairings(&[game("alice", "bob"), game("carol", "dave")])),
            ],
        )
        .await;
    heltour.rounds_of("rapid", &[ok(rounds(&[]))]).await;
    let mut syncing = Syncing::start(heltour, Duration::from_millis(50));

    assert_eq!(
        syncing.next_submitted().await,
        fixtures(&classical(), &[("alice", "bob")])
    );
    assert_eq!(
        syncing.next_submitted().await,
        fixtures(&classical(), &[("alice", "bob"), ("carol", "dave")])
    );
}

#[tokio::test]
async fn sync_carries_on_after_heltour_refuses_the_leagues() {
    let heltour = FakeHeltour(MockServer::start().await);
    heltour
        .leagues(&[ok(leagues()), ResponseTemplate::new(401)])
        .await;
    heltour
        .playing("classical", 812, &[game("alice", "bob")])
        .await;
    heltour.rounds_of("rapid", &[ok(rounds(&[]))]).await;
    let mut syncing = Syncing::start(heltour, Duration::from_millis(50));

    syncing.next_submitted().await;
    syncing.requests_reach(6).await;

    assert!(!syncing.sync.is_finished());
}

#[tokio::test]
async fn a_league_with_an_unreadable_round_submits_nothing() {
    let heltour = FakeHeltour::with_leagues().await;
    heltour
        .rounds_of(
            "classical",
            &[ok(rounds(&[current_round(812), current_round(813)]))],
        )
        .await;
    heltour
        .pairings_of("classical", 812, &[ok(pairings(&[game("alice", "bob")]))])
        .await;
    heltour
        .pairings_of("classical", 813, &[ResponseTemplate::new(502)])
        .await;
    heltour
        .playing("rapid", 900, &[game("carol", "dave")])
        .await;
    let mut syncing = Syncing::start(heltour, Duration::from_hours(1));

    assert_eq!(
        syncing.next_submitted().await,
        fixtures(&rapid(), &[("carol", "dave")])
    );
}

#[tokio::test]
async fn sync_ends_once_the_monitor_has_stopped() {
    let heltour = only_classical_playing(&[game("alice", "bob")]).await;
    let (sender, _submitted) = mpsc::unbounded_channel();
    let stopped = Recorder::spawn(Recorder(sender));
    stopped.stop_gracefully().await.unwrap();
    stopped.wait_for_shutdown().await;

    let sync = keep_in_sync(
        heltour.client(),
        stopped.recipient(),
        watch::Sender::new(ActiveLeagues::default()),
        Duration::from_hours(1),
    );

    assert!(tokio::time::timeout(PATIENCE, sync).await.is_ok());
}

#[tokio::test]
async fn fixtures_span_the_earliest_start_to_the_latest_end() {
    let heltour = FakeHeltour::with_leagues().await;
    heltour
        .rounds_of(
            "classical",
            &[ok(rounds(&[
                current_round(812),
                round(813, Some(ROUND_STARTS - 1), Some(ROUND_ENDS - 1)),
                round(814, Some(ROUND_STARTS + 1), Some(ROUND_ENDS + 1)),
            ]))],
        )
        .await;
    for (round, white, black) in [
        (812, "alice", "bob"),
        (813, "carol", "dave"),
        (814, "erin", "frank"),
    ] {
        heltour
            .pairings_of("classical", round, &[ok(pairings(&[game(white, black)]))])
            .await;
    }
    heltour
        .rounds_of(
            "rapid",
            &[ok(rounds(&[
                round(900, Some(ROUND_STARTS), None),
                round(901, None, Some(ROUND_ENDS)),
            ]))],
        )
        .await;
    for round in [900, 901] {
        heltour
            .pairings_of("rapid", round, &[ok(pairings(&[]))])
            .await;
    }
    let mut syncing = Syncing::start(heltour, Duration::from_hours(1));

    assert_eq!(
        syncing.next_submitted().await,
        NewFixtures {
            period: Period {
                starts: Some(at(ROUND_STARTS - 1)),
                ends: Some(at(ROUND_ENDS + 1)),
            },
            ..fixtures(
                &classical(),
                &[("alice", "bob"), ("carol", "dave"), ("erin", "frank")]
            )
        }
    );
    assert_eq!(syncing.next_submitted().await.period, Period::default());
}

#[tokio::test]
async fn a_board_missing_a_player_is_not_a_fixture() {
    let heltour = only_classical_playing(&[
        pairing(Some("carol"), None),
        pairing(None, Some("dave")),
        game("alice", "bob"),
    ])
    .await;
    let mut syncing = Syncing::start(heltour, Duration::from_hours(1));

    assert_eq!(
        syncing.next_submitted().await,
        fixtures(&classical(), &[("alice", "bob")])
    );
}
