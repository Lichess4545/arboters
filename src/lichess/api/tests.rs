use std::time::Duration;

use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use time::macros::datetime;
use wiremock::matchers::{body_string, header, method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockBuilder, MockServer, ResponseTemplate};

use super::*;
use crate::game_monitor::http_client;
use crate::lichess::{
    Clock, Color, Game, GameId, GameState, GameStatus, Player, Speed, UserId, Variant,
};

const GAME_LINE: &str = r#"{"id":"g1","rated":true,"variant":"standard","speed":"classical","status":20,"statusName":"started","players":{"white":{"userId":"alice"},"black":{"userId":"bob"}}}"#;
const EXPORTED_WHITE_WON: &str = r#"{"id":"g1","rated":true,"variant":"standard","speed":"classical","perf":"classical","createdAt":1793644200000,"lastMoveAt":1793650000000,"status":"mate","source":"friend","players":{"white":{"user":{"name":"Alice","id":"alice"},"rating":1900,"ratingDiff":5},"black":{"user":{"name":"Bob","id":"bob"},"rating":1850,"ratingDiff":-5}},"winner":"white","clock":{"initial":2700,"increment":45,"totalTime":4500}}"#;
const INACTIVITY_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_LINE_BYTES: usize = 1024;
const TIMEOUT: Duration = Duration::from_secs(5);

async fn lichess_answering(request: Mock, token: Option<&str>) -> (MockServer, Api) {
    let server = MockServer::start().await;
    request.mount(&server).await;
    let site = Url::parse(&server.uri()).unwrap();
    let token = token.map(SecretString::from);
    (server, Api::new(http_client().unwrap(), &site, token))
}

fn our_user_agent() -> String {
    format!(
        "arboters/{} (+https://github.com/Lichess4545/arboters)",
        env!("CARGO_PKG_VERSION")
    )
}

fn user_id(name: &str) -> UserId {
    UserId::try_new(name).unwrap()
}

fn user_ids(names: &[&str]) -> Vec<UserId> {
    names.iter().map(|name| user_id(name)).collect()
}

fn games_by_users() -> MockBuilder {
    Mock::given(method("POST")).and(path("/api/stream/games-by-users"))
}

async fn feed_streaming(body: String) -> (MockServer, GameFeed) {
    let answer = games_by_users().respond_with(ResponseTemplate::new(200).set_body_string(body));
    let (server, lichess) = lichess_answering(answer, None).await;
    let feed = lichess
        .stream_games_by_users(&user_ids(&["alice"]), INACTIVITY_TIMEOUT, MAX_LINE_BYTES)
        .await
        .unwrap();
    (server, feed)
}

#[tokio::test]
async fn connecting_posts_the_usernames_comma_separated_with_the_bearer_token_and_our_user_agent() {
    let request = games_by_users()
        .and(body_string("alice,bob"))
        .and(header(AUTHORIZATION, "Bearer lip_secret"))
        .and(header(USER_AGENT, our_user_agent().as_str()))
        .respond_with(ResponseTemplate::new(200));
    let (_server, lichess) = lichess_answering(request, Some("lip_secret")).await;

    lichess
        .stream_games_by_users(
            &user_ids(&["alice", "bob"]),
            INACTIVITY_TIMEOUT,
            MAX_LINE_BYTES,
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn a_429_stream_answer_is_reported_as_rate_limited() {
    let answer = games_by_users().respond_with(ResponseTemplate::new(429));
    let (_server, lichess) = lichess_answering(answer, None).await;
    assert!(matches!(
        lichess
            .stream_games_by_users(&user_ids(&["alice"]), INACTIVITY_TIMEOUT, MAX_LINE_BYTES)
            .await,
        Err(FeedError::RateLimited)
    ));
}

#[tokio::test]
async fn any_other_failing_stream_answer_is_reported_as_unavailable() {
    let answer = games_by_users().respond_with(ResponseTemplate::new(502));
    let (_server, lichess) = lichess_answering(answer, None).await;
    assert!(matches!(
        lichess
            .stream_games_by_users(&user_ids(&["alice"]), INACTIVITY_TIMEOUT, MAX_LINE_BYTES)
            .await,
        Err(FeedError::Unavailable(_))
    ));
}

#[tokio::test]
async fn a_game_line_streamed_by_lichess_is_a_game() {
    let (_server, mut feed) = feed_streaming(format!("{GAME_LINE}\n")).await;
    assert!(matches!(
        feed.next_event().await,
        Some(Ok(FeedEvent::Game(_)))
    ));
}

#[tokio::test]
async fn a_blank_keepalive_line_is_a_heartbeat() {
    let (_server, mut feed) = feed_streaming("\n".to_owned()).await;
    assert!(matches!(
        feed.next_event().await,
        Some(Ok(FeedEvent::Heartbeat))
    ));
}

#[tokio::test]
async fn an_undecodable_line_is_an_error() {
    let (_server, mut feed) = feed_streaming(format!("{{garbage}}\n{GAME_LINE}\n")).await;
    assert!(matches!(
        feed.next_event().await,
        Some(Err(FeedError::Undecodable { line, .. })) if line == "{garbage}"
    ));
}

#[tokio::test]
async fn an_oversized_line_is_an_error() {
    let oversized = "x".repeat(MAX_LINE_BYTES + 1);
    let (_server, mut feed) = feed_streaming(format!("{oversized}\n{GAME_LINE}\n")).await;
    assert!(matches!(
        feed.next_event().await,
        Some(Err(FeedError::LineTooLong {
            max_bytes: MAX_LINE_BYTES
        }))
    ));
}

#[tokio::test]
async fn a_stream_silent_for_the_inactivity_timeout_is_reported_inactive() {
    let silent =
        games_by_users().respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(1)));
    let (_server, lichess) = lichess_answering(silent, None).await;
    assert!(matches!(
        lichess
            .stream_games_by_users(
                &user_ids(&["alice"]),
                Duration::from_millis(50),
                MAX_LINE_BYTES
            )
            .await,
        Err(FeedError::Inactive(_))
    ));
}

fn games_of_alice() -> MockBuilder {
    Mock::given(method("GET")).and(path("/api/games/user/alice"))
}

async fn games_of_alice_against_bob(lichess: &Api) -> Result<Vec<GameState>, GamesUserError> {
    let query = GamesUserQuery {
        vs: Some(user_id("bob")),
        since: Some(datetime!(2026-11-02 18:00:00.123 UTC)),
        finished: Some(true),
        ongoing: Some(false),
    };
    lichess.games_user(&user_id("alice"), &query, TIMEOUT).await
}

#[tokio::test]
async fn the_games_of_a_user_are_asked_for_with_the_query_without_moves_as_ndjson_with_the_token_and_our_user_agent()
 {
    let request = games_of_alice()
        .and(query_param("vs", "bob"))
        .and(query_param("since", "1793642400123"))
        .and(query_param("finished", "true"))
        .and(query_param("ongoing", "false"))
        .and(query_param("moves", "false"))
        .and(header(ACCEPT, "application/x-ndjson"))
        .and(header(AUTHORIZATION, "Bearer lip_secret"))
        .and(header(USER_AGENT, our_user_agent().as_str()))
        .respond_with(ResponseTemplate::new(200))
        .expect(1);
    let (_server, lichess) = lichess_answering(request, Some("lip_secret")).await;

    assert_eq!(games_of_alice_against_bob(&lichess).await.unwrap(), []);
}

#[tokio::test]
async fn query_fields_left_unset_are_not_sent() {
    let request = games_of_alice()
        .and(query_param_is_missing("vs"))
        .and(query_param_is_missing("since"))
        .and(query_param_is_missing("finished"))
        .and(query_param_is_missing("ongoing"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1);
    let (_server, lichess) = lichess_answering(request, None).await;

    let games = lichess
        .games_user(&user_id("alice"), &GamesUserQuery::default(), TIMEOUT)
        .await;

    assert_eq!(games.unwrap(), []);
}

#[tokio::test]
async fn each_game_of_a_user_is_a_game_state() {
    let answer = games_of_alice().respond_with(
        ResponseTemplate::new(200).set_body_string(format!("{EXPORTED_WHITE_WON}\n")),
    );
    let (_server, lichess) = lichess_answering(answer, None).await;

    let player = |name: &str, rating| Player {
        user_id: user_id(name),
        rating: Some(rating),
    };
    assert_eq!(
        games_of_alice_against_bob(&lichess).await.unwrap(),
        [GameState {
            game: Game {
                id: GameId::new("g1"),
                white: player("alice", 1900),
                black: player("bob", 1850),
                speed: Speed::Classical,
                rated: true,
                variant: Variant::Standard,
                clock: Some(Clock {
                    initial_seconds: 2700,
                    increment_seconds: 45,
                }),
            },
            status: GameStatus::Mate,
            winner: Some(Color::White),
        }]
    );
}

#[tokio::test]
async fn an_undecodable_game_of_a_user_is_an_error() {
    let answer = games_of_alice().respond_with(
        ResponseTemplate::new(200).set_body_string(format!("{EXPORTED_WHITE_WON}\n{{garbage}}\n")),
    );
    let (_server, lichess) = lichess_answering(answer, None).await;

    assert!(matches!(
        games_of_alice_against_bob(&lichess).await,
        Err(GamesUserError::Undecodable { line, .. }) if line == "{garbage}"
    ));
}

#[tokio::test]
async fn a_429_games_answer_is_reported_as_rate_limited() {
    let answer = games_of_alice().respond_with(ResponseTemplate::new(429));
    let (_server, lichess) = lichess_answering(answer, None).await;

    assert!(matches!(
        games_of_alice_against_bob(&lichess).await,
        Err(GamesUserError::RateLimited)
    ));
}

#[tokio::test]
async fn the_token_is_tested_by_posting_it_and_lichess_answer_for_it_is_kept() {
    let request = Mock::given(method("POST"))
        .and(path("/api/token/test"))
        .and(header(CONTENT_TYPE, "text/plain"))
        .and(body_string("lip_secret"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"{"lip_secret": {"userId": "arbiter", "scopes": "", "expires": null}}"#,
        ))
        .expect(1);
    let (_server, lichess) = lichess_answering(request, Some("lip_secret")).await;

    assert_eq!(
        lichess.token_test().await.unwrap(),
        [Some(TokenInfo {
            user_id: user_id("arbiter")
        })]
    );
}

#[tokio::test]
async fn without_a_token_nothing_is_tested() {
    let request = Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0);
    let (_server, lichess) = lichess_answering(request, None).await;

    assert_eq!(lichess.token_test().await.unwrap(), []);
}
