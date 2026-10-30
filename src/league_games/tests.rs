use std::collections::{BTreeMap, VecDeque};
use std::convert::Infallible;
use std::sync::{Arc, Mutex};

use axum::http::header::{AUTHORIZATION, USER_AGENT};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use axum::{Form, Router};
use kameo::actor::Spawn;
use secrecy::SecretString;
use tokio::net::TcpListener;
use url::Url;

use super::*;
use crate::game_monitor::http_client;
use crate::heltour::{HeltourSettings, Pairing, league};
use crate::lichess::{self, Clock, Game, GameId, Player, Speed, Variant};

const UPDATED: &str = r#"{"updated": 1, "reversed": false, "white": "alice", "black": "bob", "game_link_changed": true, "result_changed": false}"#;

type Fields = BTreeMap<String, String>;

#[derive(Clone, Default)]
struct FakeHeltour {
    received: Arc<Mutex<Vec<(HeaderMap, Fields)>>>,
    answers: Arc<Mutex<VecDeque<(StatusCode, &'static str)>>>,
}

impl FakeHeltour {
    fn answering(answers: &[(StatusCode, &'static str)]) -> Self {
        Self {
            answers: Arc::new(Mutex::new(answers.iter().copied().collect())),
            ..Self::default()
        }
    }

    fn answer(&self, headers: HeaderMap, form: Fields) -> (StatusCode, &'static str) {
        self.received.lock().unwrap().push((headers, form));
        self.answers
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or((StatusCode::OK, UPDATED))
    }

    fn reports(&self) -> Vec<Fields> {
        let received = self.received.lock().unwrap();
        received.iter().map(|(_, form)| form.clone()).collect()
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

async fn report(heltour: &FakeHeltour, active: ActiveLeagues, events: Vec<GameEvent>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let answering = heltour.clone();
    let router = Router::new().route(
        "/api/update_pairing/",
        post(move |headers: HeaderMap, Form(form): Form<Fields>| {
            std::future::ready(answering.answer(headers, form))
        }),
    );
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let settings = HeltourSettings {
        base_url: Url::parse(&format!("http://{address}/api/")).unwrap(),
        token: Some(SecretString::from("heltour_secret")),
    };
    let reporter = Reporter::spawn((
        Reporter::new(Heltour::new(http_client().unwrap(), &settings).unwrap()),
        Monitor::spawn(Monitor).recipient(),
    ));
    reporter.ask(active).await.unwrap();
    for event in events {
        reporter.ask(event).await.unwrap();
    }
    assert!(reporter.is_alive());
}

async fn reports_of(active: ActiveLeagues, events: Vec<GameEvent>) -> Vec<Fields> {
    let heltour = FakeHeltour::default();
    report(&heltour, active, events).await;
    heltour.reports()
}

fn tag(tag: &str) -> league::Tag {
    league::Tag::try_new(tag).unwrap()
}

fn user_id(name: &str) -> lichess::UserId {
    lichess::UserId::try_new(name).unwrap()
}

const CLASSICAL_CLOCK: Clock = Clock {
    initial_seconds: 2700,
    increment_seconds: 45,
};

fn classical(tag_name: &str) -> League {
    League {
        tag: tag(tag_name),
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
    let player = |name| Player {
        user_id: user_id(name),
        rating: None,
    };
    Game {
        id: GameId::new("abcd1234"),
        white: player("alice"),
        black: player("bob"),
        speed: Speed::Classical,
        rated: true,
        variant: Variant::Standard,
        clock: Some(CLASSICAL_CLOCK),
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
        ("game_link", GameId::new("abcd1234").link()),
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
    let heltour = FakeHeltour::default();
    report(&heltour, alice_vs_bob(), vec![GameEvent::Started(game())]).await;

    let (headers, form) = heltour.received.lock().unwrap().remove(0);
    assert_eq!(form, report_to("classical", None));
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
        (StatusCode::OK, r#"{"updated": 0, "error": "not_found"}"#),
        (
            StatusCode::OK,
            r#"{"updated": 0, "error": "game_link_conflict"}"#,
        ),
        (
            StatusCode::OK,
            r#"{"updated": 0, "error": "result_already_set"}"#,
        ),
        (StatusCode::INTERNAL_SERVER_ERROR, "Server Error"),
        (StatusCode::OK, "not json"),
    ];
    let heltour = FakeHeltour::answering(&answers);
    let events = vec![GameEvent::Started(game()); answers.len() + 1];
    report(&heltour, alice_vs_bob(), events).await;

    assert_eq!(heltour.reports().len(), answers.len() + 1);
}
