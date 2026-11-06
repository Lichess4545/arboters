use std::collections::BTreeMap;

use kameo::actor::Spawn;
use secrecy::SecretString;
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

use super::*;
use crate::game_monitor::http_client;
use crate::heltour::{self, Pairing, league};
use crate::lichess::{self, Clock, Game, GameId, Variant};
use crate::testing::{self, CLASSICAL_CLOCK, tag, user_id};

const UPDATED: &str = r#"{"updated": 1, "reversed": false, "white": "alice", "black": "bob", "game_link_changed": true, "result_changed": false}"#;

type Fields = BTreeMap<String, String>;

async fn reports_of(active: ActiveLeagues, events: Vec<GameEvent>) -> Vec<Fields> {
    let heltour = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/update_pairing/"))
        .respond_with(ResponseTemplate::new(200).set_body_string(UPDATED))
        .mount(&heltour)
        .await;
    let config = heltour::Config {
        base_url: Url::parse(&format!("{}/api/", heltour.uri())).unwrap(),
        token: SecretString::from("heltour_secret"),
    };
    let reporter = Reporter::spawn((
        Reporter::new(
            Heltour::new(http_client().unwrap(), &config),
            lichess::Config::default().base_url,
        ),
        broadcast::channel(1).1,
    ));
    reporter.ask(active).await.unwrap();
    for event in events {
        reporter.ask(StreamMessage::Next(event)).await.unwrap();
    }
    assert!(reporter.is_alive());
    let requests = heltour.received_requests().await.unwrap();
    requests.iter().map(form).collect()
}

fn form(request: &Request) -> Fields {
    url::form_urlencoded::parse(&request.body)
        .into_owned()
        .collect()
}

fn classical(tag_name: &str) -> League {
    League {
        tag: tag(tag_name),
        name: league::Name::try_new(tag_name).unwrap(),
        time_control: Some(CLASSICAL_CLOCK),
        variant: Variant::Standard,
        rated: true,
    }
}

fn pairing(league: &str, white: &str, black: &str) -> Pairing {
    Pairing {
        league: tag(league),
        white: user_id(white),
        black: user_id(black),
    }
}

fn alice_vs_bob_in(league: League) -> ActiveLeagues {
    let pairing = pairing(league.tag.as_str(), "alice", "bob");
    ActiveLeagues {
        leagues: vec![league],
        pairings: vec![pairing],
    }
}

fn alice_vs_bob() -> ActiveLeagues {
    alice_vs_bob_in(classical("classical"))
}

fn game() -> Game {
    Game {
        clock: Some(CLASSICAL_CLOCK),
        ..testing::alice_vs_bob("abcd1234")
    }
}

const fn started(game: Game) -> GameEvent {
    GameEvent::Started(game)
}

fn game_where(change: impl FnOnce(&mut Game)) -> Game {
    let mut game = game();
    change(&mut game);
    game
}

fn finished(outcome: GameOutcome, ending: Ending) -> GameEvent {
    GameEvent::Finished {
        game: game(),
        outcome,
        ending,
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
        reports_of(alice_vs_bob(), vec![started(game())]).await,
        [report_to("classical", None)]
    );
}

#[tokio::test]
async fn a_valid_finish_reports_its_result() {
    let results = [
        (GameOutcome::WhiteWon, Ending::Checkmate, "1-0"),
        (GameOutcome::BlackWon, Ending::Resignation, "0-1"),
        (GameOutcome::BlackWon, Ending::OutOfTime, "0-1"),
        (GameOutcome::Draw, Ending::Draw, "1/2-1/2"),
        (GameOutcome::Draw, Ending::Stalemate, "1/2-1/2"),
    ];
    for (outcome, ending, result) in results {
        assert_eq!(
            reports_of(alice_vs_bob(), vec![finished(outcome, ending)]).await,
            [report_to("classical", Some(result))],
            "{ending:?}"
        );
    }
}

struct Unreported(&'static str, ActiveLeagues, GameEvent);

#[tokio::test]
async fn an_invalid_game_is_not_reported() {
    let clock = |initial_seconds, increment_seconds| {
        Some(Clock {
            initial_seconds,
            increment_seconds,
        })
    };
    let without_time_controls = alice_vs_bob_in(League {
        time_control: None,
        ..classical("classical")
    });
    let breaches = [
        Unreported(
            "initial time",
            alice_vs_bob(),
            started(game_where(|game| game.clock = clock(1800, 45))),
        ),
        Unreported(
            "increment",
            alice_vs_bob(),
            started(game_where(|game| game.clock = clock(2700, 30))),
        ),
        Unreported(
            "no clock",
            alice_vs_bob(),
            started(game_where(|game| game.clock = None)),
        ),
        Unreported(
            "no time controls",
            without_time_controls,
            started(game_where(|game| game.clock = None)),
        ),
        Unreported(
            "unrated",
            alice_vs_bob(),
            started(game_where(|game| game.rated = false)),
        ),
        Unreported(
            "variant",
            alice_vs_bob(),
            started(game_where(|game| game.variant = Variant::Chess960)),
        ),
        Unreported(
            "claimed victory",
            alice_vs_bob(),
            finished(GameOutcome::WhiteWon, Ending::VictoryClaimed),
        ),
        Unreported(
            "cheat detected",
            alice_vs_bob(),
            finished(GameOutcome::BlackWon, Ending::CheatDetected),
        ),
        Unreported(
            "aborted",
            alice_vs_bob(),
            finished(GameOutcome::Aborted, Ending::Aborted),
        ),
        Unreported(
            "never started",
            alice_vs_bob(),
            finished(GameOutcome::Aborted, Ending::NeverStarted),
        ),
        Unreported(
            "unknown finish",
            alice_vs_bob(),
            finished(GameOutcome::WhiteWon, Ending::Unknown),
        ),
    ];
    for Unreported(name, active, event) in breaches {
        assert_eq!(reports_of(active, vec![event]).await, [], "{name}");
    }
}

#[tokio::test]
async fn a_game_is_reported_with_its_own_colors_to_each_league_that_pairs_its_players() {
    let active = ActiveLeagues {
        leagues: vec![
            classical("classical"),
            classical("rapid"),
            classical("blitz"),
        ],
        pairings: vec![
            pairing("classical", "alice", "carol"),
            pairing("rapid", "bob", "alice"),
            pairing("blitz", "alice", "bob"),
        ],
    };
    assert_eq!(
        reports_of(active, vec![started(game())]).await,
        [report_to("rapid", None), report_to("blitz", None)]
    );
}
