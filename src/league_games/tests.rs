use std::collections::BTreeMap;
use std::time::Duration;

use kameo::actor::Spawn;
use secrecy::SecretString;
use url::Url;
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockBuilder, MockServer, Request, ResponseTemplate};

use super::*;
use crate::game_monitor::http_client;
use crate::game_monitor::{FixturesId, MatchedFixture};
use crate::heltour;
use crate::lichess::{self, Clock, Game, GameId, Variant};
use crate::testing::{CLASSICAL_CLOCK, alice_vs_bob, fixture, league, tag};

const UPDATED: &str = r#"{"updated": 1, "reversed": false, "white": "alice", "black": "bob", "game_link_changed": true, "result_changed": false}"#;

const PARALLEL_REPORTS: u32 = 2;

type Fields = BTreeMap<String, String>;

fn reporter(heltour: &MockServer) -> Reporter {
    let config = heltour::Config {
        base_url: Url::parse(&format!("{}/api/", heltour.uri())).unwrap(),
        token: SecretString::from("heltour_secret"),
        retry: heltour::RetryConfig::default(),
    };
    Reporter::new(
        Heltour::new(http_client().unwrap(), &config),
        lichess::Config::default().base_url,
        NonZeroUsize::new(PARALLEL_REPORTS as usize).unwrap(),
    )
}

fn update_pairing() -> MockBuilder {
    Mock::given(method("POST")).and(path("/api/update_pairing/"))
}

fn updated() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_string(UPDATED)
}

async fn reports_of(events: Vec<GameEvent>) -> Vec<Fields> {
    let heltour = MockServer::start().await;
    update_pairing()
        .respond_with(updated())
        .mount(&heltour)
        .await;
    let reporter = reporter(&heltour);
    let reports = Arc::clone(&reporter.parallel_reports);
    let reporter = Reporter::spawn((reporter, broadcast::channel(1).1));
    for event in events {
        reporter.ask(StreamMessage::Next(event)).await.unwrap();
    }
    let _all_finished = reports.acquire_many(PARALLEL_REPORTS).await.unwrap();
    assert!(reporter.is_alive());
    let requests = heltour.received_requests().await.unwrap();
    requests.iter().map(form).collect()
}

fn form(request: &Request) -> Fields {
    url::form_urlencoded::parse(&request.body)
        .into_owned()
        .collect()
}

fn fixture_in(league: &str, white: &str, black: &str) -> MatchedFixture {
    MatchedFixture {
        fixtures_id: FixturesId::new(1),
        league: tag(league).monitored(),
        fixture: fixture(white, black),
    }
}

fn started(game: Game) -> GameEvent {
    GameEvent {
        change: GameChange::Started(game),
        matched: vec![fixture_in("classical", "alice", "bob")],
    }
}

fn game() -> Game {
    Game {
        clock: Some(CLASSICAL_CLOCK),
        ..alice_vs_bob("abcd1234")
    }
}

fn game_where(change: impl FnOnce(&mut Game)) -> Game {
    let mut game = game();
    change(&mut game);
    game
}

fn decided(decision: Decision) -> GameEvent {
    finished(GameChange::Decided {
        game: game(),
        decision,
    })
}

fn void(reason: VoidReason) -> GameEvent {
    finished(GameChange::Void {
        game: game(),
        reason,
    })
}

fn finished(change: GameChange) -> GameEvent {
    GameEvent {
        change,
        matched: vec![fixture_in("classical", "alice", "bob")],
    }
}

fn report_to(league: &str, result: Option<&str>) -> Fields {
    let game_link = GameId::new("abcd1234").link(&lichess::Config::default().base_url);
    [
        ("league", league),
        ("white", "alice"),
        ("black", "bob"),
        ("game_link", game_link.as_str()),
        ("strict", "true"),
    ]
    .into_iter()
    .chain(result.map(|result| ("result", result)))
    .map(|(key, value)| (key.to_owned(), value.to_owned()))
    .collect()
}

#[tokio::test]
async fn a_valid_start_is_reported_without_a_result() {
    assert_eq!(
        reports_of(vec![started(game())]).await,
        [report_to("classical", None)]
    );
}

#[tokio::test]
async fn a_valid_finish_reports_its_result() {
    let results = [
        (Decision::WhiteWon(Win::Checkmate), "1-0"),
        (Decision::BlackWon(Win::Resignation), "0-1"),
        (Decision::BlackWon(Win::OutOfTime), "0-1"),
        (Decision::Drawn(Draw::AgreementOrRule), "1/2-1/2"),
        (Decision::Drawn(Draw::Stalemate), "1/2-1/2"),
    ];
    for (decision, result) in results {
        assert_eq!(
            reports_of(vec![decided(decision)]).await,
            [report_to("classical", Some(result))],
            "{decision:?}"
        );
    }
}

struct Unreported(&'static str, GameEvent);

#[tokio::test]
async fn an_invalid_game_is_not_reported() {
    let clock = |initial_seconds, increment_seconds| {
        Some(Clock {
            initial_seconds,
            increment_seconds,
        })
    };
    let mut without_time_controls = started(game_where(|game| game.clock = None));
    without_time_controls.matched[0].fixture.time_control = None;
    let breaches = [
        Unreported(
            "initial time",
            started(game_where(|game| game.clock = clock(1800, 45))),
        ),
        Unreported(
            "increment",
            started(game_where(|game| game.clock = clock(2700, 30))),
        ),
        Unreported("no clock", started(game_where(|game| game.clock = None))),
        Unreported("no time controls", without_time_controls),
        Unreported("unrated", started(game_where(|game| game.rated = false))),
        Unreported(
            "variant",
            started(game_where(|game| game.variant = Variant::Chess960)),
        ),
        Unreported(
            "claimed victory",
            decided(Decision::WhiteWon(Win::VictoryClaimed)),
        ),
        Unreported(
            "cheat detected",
            decided(Decision::BlackWon(Win::CheatDetected)),
        ),
        Unreported("aborted", void(VoidReason::Aborted)),
        Unreported("never started", void(VoidReason::NeverStarted)),
        Unreported("unknown win", decided(Decision::WhiteWon(Win::Unknown))),
        Unreported("unknown draw", decided(Decision::Drawn(Draw::Unknown))),
        Unreported("player left", decided(Decision::Drawn(Draw::PlayerLeft))),
    ];
    for Unreported(name, event) in breaches {
        assert_eq!(reports_of(vec![event]).await, [], "{name}");
    }
}

#[tokio::test]
async fn every_fitting_league_gets_the_game_in_its_colors() {
    let mut unrated = fixture_in("classical", "alice", "bob");
    unrated.fixture.rated = false;
    let event = GameEvent {
        change: GameChange::Started(game()),
        matched: vec![
            unrated,
            fixture_in("rapid", "bob", "alice"),
            fixture_in("blitz", "alice", "bob"),
        ],
    };
    assert_eq!(
        reports_of(vec![event]).await,
        [report_to("rapid", None), report_to("blitz", None)]
    );
}

#[tokio::test]
async fn a_league_from_another_source_is_not_reported() {
    let event = GameEvent {
        change: GameChange::Started(game()),
        matched: vec![
            MatchedFixture {
                league: league("sheets", "rapid"),
                ..fixture_in("rapid", "alice", "bob")
            },
            fixture_in("classical", "alice", "bob"),
        ],
    };
    assert_eq!(
        reports_of(vec![event]).await,
        [report_to("classical", None)]
    );
}

#[tokio::test]
async fn a_slow_report_does_not_hold_up_the_next_game() {
    let heltour = MockServer::start().await;
    update_pairing()
        .and(body_string_contains("slow1234"))
        .respond_with(updated().set_delay(Duration::from_mins(1)))
        .mount(&heltour)
        .await;
    let next = update_pairing()
        .and(body_string_contains("next1234"))
        .respond_with(updated())
        .expect(1)
        .mount_as_scoped(&heltour)
        .await;
    let reporter = Reporter::spawn((reporter(&heltour), broadcast::channel(1).1));
    let both_reported = async {
        for id in ["slow1234", "next1234"] {
            let game = Game {
                clock: Some(CLASSICAL_CLOCK),
                ..alice_vs_bob(id)
            };
            reporter
                .ask(StreamMessage::Next(started(game)))
                .await
                .unwrap();
        }
        next.wait_until_satisfied().await;
    };
    tokio::time::timeout(Duration::from_secs(5), both_reported)
        .await
        .unwrap();
}
