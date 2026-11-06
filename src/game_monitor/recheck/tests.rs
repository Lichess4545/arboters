use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use kameo::actor::Spawn;
use time::macros::datetime;
use tokio::sync::mpsc;
use url::Url;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockBuilder, MockServer, Request, Respond, ResponseTemplate};

use super::*;
use crate::game_monitor::{FixturesId, Period, http_client};
use crate::lichess::GameState;
use crate::testing::{Collector, alice_beat_bob, fixture, league};

const PATIENCE: Duration = Duration::from_secs(5);
const EXPORTED_WHITE_WON: &str = r#"{"id":"g1","rated":true,"variant":"standard","speed":"classical","createdAt":1793644200000,"lastMoveAt":1793650000000,"status":"mate","players":{"white":{"user":{"name":"Alice","id":"alice"},"rating":1900},"black":{"user":{"name":"Bob","id":"bob"},"rating":1850}},"winner":"white","clock":{"initial":2700,"increment":45,"totalTime":4500}}"#;

fn stored_fixtures(starts: Option<OffsetDateTime>, games: &[Fixture]) -> Fixtures {
    Fixtures {
        id: FixturesId::new(1),
        league: league("heltour", "team4545"),
        period: Period { starts, ends: None },
        games: games.iter().cloned().collect::<BTreeSet<_>>(),
        stored_at: datetime!(2026-11-03 09:30:00 UTC),
    }
}

fn recheck_config() -> Config {
    Config {
        interval: Duration::from_hours(1),
        rate_limit_backoff: Duration::from_millis(300),
    }
}

fn start_rechecking(
    lichess: &MockServer,
    active: Vec<Fixtures>,
) -> mpsc::UnboundedReceiver<GameState> {
    let site = Url::parse(&lichess.uri()).unwrap();
    let (_, active) = watch::channel(active);
    let (handed, games) = mpsc::unbounded_channel();
    tokio::spawn(keep_rechecking(
        lichess::Api::new(http_client().unwrap(), &site, None),
        active,
        Collector::spawn(Collector(handed)).recipient(),
        recheck_config(),
    ));
    games
}

async fn next<T>(received: &mut mpsc::UnboundedReceiver<T>) -> T {
    tokio::time::timeout(PATIENCE, received.recv())
        .await
        .unwrap()
        .unwrap()
}

fn games_of(white: &str) -> MockBuilder {
    Mock::given(method("GET")).and(path(format!("/api/games/user/{white}")))
}

fn games_between_since(white: &str, black: &str, since: &str) -> MockBuilder {
    games_of(white)
        .and(query_param("vs", black))
        .and(query_param("since", since))
}

#[tokio::test]
async fn each_fixtures_finished_games_are_handed_to_the_monitor() {
    let lichess = MockServer::start().await;
    games_between_since("carol", "dave", "1793698200000")
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&lichess)
        .await;
    games_between_since("alice", "bob", "1793642400123")
        .respond_with(ResponseTemplate::new(200).set_body_string(format!("{EXPORTED_WHITE_WON}\n")))
        .expect(1)
        .mount(&lichess)
        .await;

    let mut games = start_rechecking(
        &lichess,
        vec![
            stored_fixtures(None, &[fixture("carol", "dave")]),
            stored_fixtures(
                Some(datetime!(2026-11-02 18:00:00.123 UTC)),
                &[fixture("alice", "bob")],
            ),
        ],
    );

    assert_eq!(next(&mut games).await, alice_beat_bob());
    lichess.verify().await;
}

fn answering_at(arrivals: &mpsc::UnboundedSender<Instant>, status: u16) -> impl Respond + use<> {
    let arrivals = arrivals.clone();
    move |_: &Request| {
        arrivals.send(Instant::now()).unwrap();
        ResponseTemplate::new(status)
    }
}

#[tokio::test]
async fn only_a_429_delays_the_next_request() {
    let lichess = MockServer::start().await;
    let (arrived, mut arrivals) = mpsc::unbounded_channel();
    for (white, status) in [("alice", 200), ("carol", 429), ("erin", 200)] {
        games_of(white)
            .respond_with(answering_at(&arrived, status))
            .mount(&lichess)
            .await;
    }

    let _games = start_rechecking(
        &lichess,
        vec![stored_fixtures(
            None,
            &[
                fixture("alice", "bob"),
                fixture("carol", "dave"),
                fixture("erin", "frank"),
            ],
        )],
    );

    let alice = next(&mut arrivals).await;
    let carol = next(&mut arrivals).await;
    let erin = next(&mut arrivals).await;
    assert!(carol - alice < recheck_config().rate_limit_backoff);
    assert!(erin - carol >= recheck_config().rate_limit_backoff);
}
