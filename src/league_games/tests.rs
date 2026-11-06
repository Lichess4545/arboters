use std::collections::BTreeMap;
use std::convert::Infallible;

use kameo::actor::Spawn;
use reqwest::header::{AUTHORIZATION, USER_AGENT};
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

async fn report(
    answers: &[(u16, &str)],
    active: ActiveLeagues,
    events: Vec<GameEvent>,
) -> Vec<Request> {
    let heltour = MockServer::start().await;
    let update_pairing = || Mock::given(method("POST")).and(path("/api/update_pairing/"));
    for (status, body) in answers {
        update_pairing()
            .respond_with(ResponseTemplate::new(*status).set_body_string(*body))
            .up_to_n_times(1)
            .mount(&heltour)
            .await;
    }
    update_pairing()
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
        Monitor::spawn(Monitor).recipient(),
    ));
    reporter.ask(active).await.unwrap();
    for event in events {
        reporter.ask(event).await.unwrap();
    }
    assert!(reporter.is_alive());
    heltour.received_requests().await.unwrap()
}

fn form(request: &Request) -> Fields {
    url::form_urlencoded::parse(&request.body)
        .into_owned()
        .collect()
}

async fn reports_of(active: ActiveLeagues, events: Vec<GameEvent>) -> Vec<Fields> {
    report(&[], active, events).await.iter().map(form).collect()
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

const fn finished(game: Game, outcome: GameOutcome, ending: Ending) -> GameEvent {
    GameEvent::Finished {
        game,
        outcome,
        ending,
    }
}

fn report_to(league: &str, result: Option<&str>) -> Fields {
    let mut fields: Fields = [
        ("league", league.to_owned()),
        ("white", "alice".to_owned()),
        ("black", "bob".to_owned()),
        (
            "game_link",
            GameId::new("abcd1234")
                .link(&lichess::Config::default().base_url)
                .to_string(),
        ),
        ("strict", "true".to_owned()),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), value))
    .collect();
    if let Some(result) = result {
        fields.insert("result".to_owned(), result.to_owned());
    }
    fields
}

#[tokio::test]
async fn a_valid_start_is_reported_strictly_with_its_game_link_and_our_credentials() {
    let requests = report(&[], alice_vs_bob(), vec![GameEvent::Started(game())]).await;

    let headers = &requests[0].headers;
    assert_eq!(form(&requests[0]), report_to("classical", None));
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
            reports_of(alice_vs_bob(), vec![finished(game(), outcome, ending)]).await,
            [report_to("classical", Some(result))],
            "{ending:?}"
        );
    }
}

#[tokio::test]
async fn a_game_that_breaks_its_leagues_rules_is_not_reported() {
    let clock = |initial_seconds, increment_seconds| {
        Some(Clock {
            initial_seconds,
            increment_seconds,
        })
    };
    let breaches = [
        (
            "initial time",
            alice_vs_bob(),
            GameEvent::Started(Game {
                clock: clock(1800, 45),
                ..game()
            }),
        ),
        (
            "increment",
            alice_vs_bob(),
            GameEvent::Started(Game {
                clock: clock(2700, 30),
                ..game()
            }),
        ),
        (
            "no clock",
            alice_vs_bob(),
            GameEvent::Started(Game {
                clock: None,
                ..game()
            }),
        ),
        (
            "a league without a time control",
            alice_vs_bob_in(League {
                time_control: None,
                ..classical("classical")
            }),
            GameEvent::Started(Game {
                clock: None,
                ..game()
            }),
        ),
        (
            "unrated",
            alice_vs_bob(),
            GameEvent::Started(Game {
                rated: false,
                ..game()
            }),
        ),
        (
            "variant",
            alice_vs_bob(),
            GameEvent::Started(Game {
                variant: Variant::Chess960,
                ..game()
            }),
        ),
        (
            "claimed victory",
            alice_vs_bob(),
            finished(game(), GameOutcome::WhiteWon, Ending::VictoryClaimed),
        ),
        (
            "cheat detected",
            alice_vs_bob(),
            finished(game(), GameOutcome::BlackWon, Ending::CheatDetected),
        ),
        (
            "aborted",
            alice_vs_bob(),
            finished(game(), GameOutcome::Aborted, Ending::Aborted),
        ),
        (
            "never started",
            alice_vs_bob(),
            finished(game(), GameOutcome::Aborted, Ending::NeverStarted),
        ),
        (
            "unknown finish",
            alice_vs_bob(),
            finished(game(), GameOutcome::WhiteWon, Ending::Unknown),
        ),
    ];
    for (breach, active, event) in breaches {
        assert_eq!(reports_of(active, vec![event]).await, [], "{breach}");
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
        reports_of(active, vec![GameEvent::Started(game())]).await,
        [report_to("rapid", None), report_to("blitz", None)]
    );
}

#[tokio::test]
async fn the_reporter_carries_on_after_heltour_declines_or_fails() {
    let answers = [
        (200, r#"{"updated": 0, "error": "not_found"}"#),
        (200, r#"{"updated": 0, "error": "game_link_conflict"}"#),
        (200, r#"{"updated": 0, "error": "result_already_set"}"#),
        (500, "Server Error"),
        (200, "not json"),
    ];
    let events = vec![GameEvent::Started(game()); answers.len() + 1];
    let reports = report(&answers, alice_vs_bob(), events).await;

    assert_eq!(reports.len(), answers.len() + 1);
}
