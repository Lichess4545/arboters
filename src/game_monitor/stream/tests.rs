use reqwest::header::{AUTHORIZATION, USER_AGENT};
use wiremock::matchers::{body_string, header, method, path};
use wiremock::{Mock, MockBuilder, MockServer, ResponseTemplate};

use super::*;
use crate::game_monitor::http_client;
use crate::lichess::{GameId, Player, Speed, Variant};

const GAME_LINE: &str = r#"{"id":"g1","rated":true,"variant":"standard","speed":"classical","status":20,"players":{"white":{"userId":"alice"},"black":{"userId":"bob"}}}"#;
const INACTIVITY_TIMEOUT: Duration = Duration::from_secs(60);

async fn lichess_answering(request: Mock, token: Option<&str>) -> (MockServer, LichessStream) {
    let server = MockServer::start().await;
    request.mount(&server).await;
    let config = lichess::Config {
        base_url: Url::parse(&server.uri()).unwrap(),
        token: token.map(SecretString::from),
        ..lichess::Config::default()
    };
    (server, LichessStream::new(http_client().unwrap(), &config))
}

fn games_by_users() -> MockBuilder {
    Mock::given(method("POST")).and(path("/api/stream/games-by-users"))
}

fn user_ids(names: &[&str]) -> Vec<UserId> {
    names
        .iter()
        .map(|name| UserId::try_new(*name).unwrap())
        .collect()
}

fn feed_of(lines: Vec<Result<&'static str, LinesCodecError>>) -> LichessFeed {
    let lines = futures::stream::iter(lines)
        .map(|line| line.map(str::to_owned))
        .chain(futures::stream::pending());
    LichessFeed {
        lines: Box::pin(lines),
        inactivity_timeout: INACTIVITY_TIMEOUT,
    }
}

#[tokio::test]
async fn connecting_posts_the_usernames_comma_separated_with_the_bearer_token_and_our_user_agent() {
    let user_agent = format!(
        "arboters/{} (+https://github.com/Lichess4545/arboters)",
        env!("CARGO_PKG_VERSION")
    );
    let request = games_by_users()
        .and(body_string("alice,bob"))
        .and(header(AUTHORIZATION, "Bearer lip_secret"))
        .and(header(USER_AGENT, user_agent.as_str()))
        .respond_with(ResponseTemplate::new(200));
    let (_server, lichess) = lichess_answering(request, Some("lip_secret")).await;

    lichess.connect(&user_ids(&["alice", "bob"])).await.unwrap();
}

#[tokio::test]
async fn a_429_answer_is_reported_as_rate_limited() {
    let answer = games_by_users().respond_with(ResponseTemplate::new(429));
    let (_server, lichess) = lichess_answering(answer, None).await;
    assert!(matches!(
        lichess.connect(&user_ids(&["alice"])).await,
        Err(FeedError::RateLimited)
    ));
}

#[tokio::test]
async fn any_other_failing_answer_is_reported_as_unavailable() {
    let answer = games_by_users().respond_with(ResponseTemplate::new(502));
    let (_server, lichess) = lichess_answering(answer, None).await;
    assert!(matches!(
        lichess.connect(&user_ids(&["alice"])).await,
        Err(FeedError::Unavailable(_))
    ));
}

#[tokio::test]
async fn a_game_line_streamed_by_lichess_is_observed() {
    let answer = games_by_users()
        .respond_with(ResponseTemplate::new(200).set_body_string(format!("{GAME_LINE}\n")));
    let (_server, lichess) = lichess_answering(answer, None).await;
    let mut feed = lichess.connect(&user_ids(&["alice"])).await.unwrap();
    assert!(matches!(
        feed.next_event().await,
        Some(Ok(FeedEvent::Observed(_)))
    ));
}

#[tokio::test(start_paused = true)]
async fn a_blank_keepalive_line_is_a_heartbeat() {
    let mut feed = feed_of(vec![Ok("")]);
    assert!(matches!(
        feed.next_event().await,
        Some(Ok(FeedEvent::Heartbeat))
    ));
}

#[tokio::test(start_paused = true)]
async fn an_undecodable_line_is_skipped() {
    let mut feed = feed_of(vec![Ok("{garbage}"), Ok(GAME_LINE)]);
    assert!(matches!(
        feed.next_event().await,
        Some(Ok(FeedEvent::Observed(_)))
    ));
}

#[tokio::test(start_paused = true)]
async fn an_oversized_line_is_skipped() {
    let mut feed = feed_of(vec![
        Err(LinesCodecError::MaxLineLengthExceeded),
        Ok(GAME_LINE),
    ]);
    assert!(matches!(
        feed.next_event().await,
        Some(Ok(FeedEvent::Observed(_)))
    ));
}

#[tokio::test(start_paused = true)]
async fn a_feed_silent_for_the_inactivity_timeout_is_reported_inactive() {
    let mut feed = feed_of(vec![]);
    let outcome = tokio::time::timeout(INACTIVITY_TIMEOUT * 2, feed.next_event()).await;
    assert!(matches!(outcome, Ok(Some(Err(FeedError::Inactive(_))))));
}

#[test]
fn a_created_game_is_not_an_event() {
    let player = |name: &str| Player {
        user_id: UserId::try_new(name).unwrap(),
        rating: None,
    };
    let created = GameObservation {
        game: Game {
            id: GameId::new("g1"),
            white: player("alice"),
            black: player("bob"),
            speed: Speed::Classical,
            rated: true,
            variant: Variant::Standard,
            clock: None,
        },
        phase: GamePhase::Created,
    };
    assert_eq!(created.into_event(), None);
}

fn stream_with_token(token: Option<&str>) -> LichessStream {
    let config = lichess::Config {
        token: token.map(SecretString::from),
        ..lichess::Config::default()
    };
    LichessStream::new(http_client().unwrap(), &config)
}

#[test]
fn without_a_token_a_connection_watches_up_to_300_users() {
    let stream = stream_with_token(None);
    assert_eq!(stream.users_per_connection(None), 300);
}

#[test]
fn with_a_token_not_known_to_be_lichess4545s_a_connection_watches_up_to_500_users() {
    let stream = stream_with_token(Some("lip_secret"));
    let arbiter = UserId::try_new("arbiter").unwrap();
    assert_eq!(
        [None, Some(&arbiter)].map(|owner| stream.users_per_connection(owner)),
        [500; 2]
    );
}

#[test]
fn with_lichess4545s_token_a_connection_watches_up_to_900_users() {
    let stream = stream_with_token(Some("lip_secret"));
    let lichess4545 = UserId::try_new("lichess4545").unwrap();
    assert_eq!(stream.users_per_connection(Some(&lichess4545)), 900);
}
