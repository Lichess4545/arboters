use std::sync::{Arc, Mutex};

use axum::Router;
use axum::http::HeaderMap;
use axum::http::header::{AUTHORIZATION, USER_AGENT};
use axum::routing::{MethodRouter, post};
use tokio::net::TcpListener;

use super::*;
use crate::game_monitor::http_client;
use crate::lichess::{GameId, Player, Speed};

const GAME_LINE: &str = r#"{"id":"g1","rated":true,"speed":"classical","status":20,"players":{"white":{"userId":"alice"},"black":{"userId":"bob"}}}"#;
const INACTIVITY_TIMEOUT: Duration = Duration::from_secs(60);

async fn lichess_answering(answer: MethodRouter, token: Option<&str>) -> LichessStream {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = Router::new().route("/api/stream/games-by-users", answer);
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let settings = LichessSettings {
        base_url: Url::parse(&format!("http://{address}")).unwrap(),
        token: token.map(SecretString::from),
        ..LichessSettings::default()
    };
    LichessStream::new(http_client().unwrap(), &settings).unwrap()
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
    let received = Arc::new(Mutex::new(None));
    let recorder = Arc::clone(&received);
    let answer = post(move |headers: HeaderMap, body: String| {
        let header = |name| headers[name].to_str().unwrap().to_owned();
        *recorder.lock().unwrap() = Some((body, header(AUTHORIZATION), header(USER_AGENT)));
        async {}
    });
    let lichess = lichess_answering(answer, Some("lip_secret")).await;

    lichess.connect(&user_ids(&["alice", "bob"])).await.unwrap();

    let expected = (
        "alice,bob".to_owned(),
        "Bearer lip_secret".to_owned(),
        format!(
            "arboters/{} (+https://github.com/Lichess4545/arboters)",
            env!("CARGO_PKG_VERSION")
        ),
    );
    assert_eq!(*received.lock().unwrap(), Some(expected));
}

#[tokio::test]
async fn a_429_answer_is_reported_as_rate_limited() {
    let answer = post(|| async { StatusCode::TOO_MANY_REQUESTS });
    let lichess = lichess_answering(answer, None).await;
    assert!(matches!(
        lichess.connect(&user_ids(&["alice"])).await,
        Err(FeedError::RateLimited)
    ));
}

#[tokio::test]
async fn any_other_failing_answer_is_reported_as_unavailable() {
    let answer = post(|| async { StatusCode::BAD_GATEWAY });
    let lichess = lichess_answering(answer, None).await;
    assert!(matches!(
        lichess.connect(&user_ids(&["alice"])).await,
        Err(FeedError::Unavailable(_))
    ));
}

#[tokio::test]
async fn a_game_line_streamed_by_lichess_is_observed() {
    let answer = post(|| async { format!("{GAME_LINE}\n") });
    let lichess = lichess_answering(answer, None).await;
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
        },
        phase: GamePhase::Created,
    };
    assert_eq!(created.into_event(), None);
}

fn stream_with_token(token: Option<&str>) -> LichessStream {
    let settings = LichessSettings {
        token: token.map(SecretString::from),
        ..LichessSettings::default()
    };
    LichessStream::new(http_client().unwrap(), &settings).unwrap()
}

fn owned_by(name: &str) -> TokenState {
    TokenState::Valid {
        owner: UserId::try_new(name).unwrap(),
    }
}

#[test]
fn without_a_token_a_connection_watches_up_to_300_users() {
    let stream = stream_with_token(None);
    assert_eq!(stream.users_per_connection(&TokenState::Untested), 300);
}

#[test]
fn with_a_token_not_known_to_be_lichess4545s_a_connection_watches_up_to_500_users() {
    let stream = stream_with_token(Some("lip_secret"));
    let tokens = [
        TokenState::Untested,
        TokenState::Invalid,
        owned_by("arbiter"),
    ];
    assert_eq!(
        tokens.map(|token| stream.users_per_connection(&token)),
        [500; 3]
    );
}

#[test]
fn with_lichess4545s_token_a_connection_watches_up_to_900_users() {
    let stream = stream_with_token(Some("lip_secret"));
    assert_eq!(stream.users_per_connection(&owned_by("lichess4545")), 900);
}
