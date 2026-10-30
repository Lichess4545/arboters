use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::Router;
use axum::http::header::USER_AGENT;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use kameo::actor::Spawn;
use tokio::net::TcpListener;

use super::*;
use crate::game_monitor::http_client;

const PATIENCE: Duration = Duration::from_secs(5);
const VALID: &str = r#"{"lip_secret": {"userId": "arbiter", "scopes": "", "expires": null}}"#;
const INVALID: &str = r#"{"lip_secret": null}"#;

struct Request {
    body: String,
    headers: HeaderMap,
}

#[derive(Clone, Default)]
struct FakeLichess {
    requests: Arc<Mutex<Vec<Request>>>,
    answers: Arc<Mutex<VecDeque<(StatusCode, &'static str)>>>,
}

impl FakeLichess {
    fn answer(&self, body: String, headers: HeaderMap) -> (StatusCode, &'static str) {
        self.requests
            .lock()
            .unwrap()
            .push(Request { body, headers });
        let mut answers = self.answers.lock().unwrap();
        if answers.len() > 1 {
            answers.pop_front().unwrap()
        } else {
            answers[0]
        }
    }

    fn request_count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }
}

struct Checking {
    lichess: FakeLichess,
    state: watch::Receiver<TokenState>,
    _check: ActorRef<TokenCheck>,
}

async fn checking_against(answers: &[(StatusCode, &'static str)]) -> Checking {
    let lichess = FakeLichess {
        answers: Arc::new(Mutex::new(answers.iter().copied().collect())),
        ..FakeLichess::default()
    };
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let answering = lichess.clone();
    let router = Router::new().route(
        "/api/token/test",
        post(move |headers: HeaderMap, body: String| {
            std::future::ready(answering.answer(body, headers))
        }),
    );
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let settings = LichessSettings {
        base_url: Url::parse(&format!("http://{address}")).unwrap(),
        token_check_interval: Duration::from_millis(50),
        ..LichessSettings::default()
    };
    let (check, state) = TokenCheck::new(
        http_client().unwrap(),
        &settings,
        SecretString::from("lip_secret"),
    )
    .unwrap();
    Checking {
        lichess,
        state,
        _check: TokenCheck::spawn(check),
    }
}

async fn becomes(state: &mut watch::Receiver<TokenState>, expected: TokenState) {
    tokio::time::timeout(PATIENCE, state.wait_for(|state| *state == expected))
        .await
        .unwrap()
        .unwrap();
}

async fn eventually(what: &str, condition: impl Fn() -> bool) {
    let deadline = Instant::now() + PATIENCE;
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting until {what}");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

#[tokio::test]
async fn the_token_is_sent_for_testing_with_our_user_agent() {
    let checking = checking_against(&[(StatusCode::OK, VALID)]).await;
    eventually("the token is tested", || {
        checking.lichess.request_count() >= 1
    })
    .await;

    let Request { body, headers } = checking.lichess.requests.lock().unwrap().remove(0);
    assert_eq!(body, "lip_secret");
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
async fn a_token_lichess_reports_invalid_is_invalid() {
    let mut checking = checking_against(&[(StatusCode::OK, INVALID)]).await;
    becomes(&mut checking.state, TokenState::Invalid).await;
}

#[tokio::test]
async fn the_token_is_tested_again_regularly() {
    let mut checking =
        checking_against(&[(StatusCode::OK, INVALID), (StatusCode::OK, VALID)]).await;
    becomes(&mut checking.state, TokenState::Invalid).await;
    becomes(
        &mut checking.state,
        TokenState::Valid {
            owner: lichess::UserId::try_new("arbiter").unwrap(),
        },
    )
    .await;
}

#[tokio::test]
async fn a_failed_test_keeps_the_previous_state() {
    let mut checking = checking_against(&[
        (StatusCode::OK, INVALID),
        (StatusCode::BAD_GATEWAY, "Bad Gateway"),
    ])
    .await;
    becomes(&mut checking.state, TokenState::Invalid).await;
    eventually("lichess has failed twice", || {
        checking.lichess.request_count() >= 3
    })
    .await;
    assert_eq!(*checking.state.borrow(), TokenState::Invalid);
}

#[tokio::test]
async fn until_lichess_answers_the_token_is_untested() {
    let checking = checking_against(&[(StatusCode::BAD_GATEWAY, "Bad Gateway")]).await;
    eventually("lichess has failed twice", || {
        checking.lichess.request_count() >= 2
    })
    .await;
    assert_eq!(*checking.state.borrow(), TokenState::Untested);
}
