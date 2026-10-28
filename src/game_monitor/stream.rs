use std::fmt;
use std::pin::Pin;
use std::time::Duration;

use futures::{Stream, StreamExt, TryStreamExt};
use reqwest::StatusCode;
use reqwest::header::CONTENT_TYPE;
use secrecy::{ExposeSecret, SecretString};
use tokio_util::codec::{FramedRead, LinesCodec, LinesCodecError};
use tokio_util::io::StreamReader;
use url::Url;

use crate::lichess::{Game, UserId};

use super::event::{GameEvent, GameOutcome};
use super::settings::LichessSettings;

mod json;

const GAMES_BY_USERS_PATH: &str = "api/stream/games-by-users";
const MAX_LINE_BYTES: usize = 64 * 1024;
const ANONYMOUS_USERS_PER_CONNECTION: usize = 300;
const LICHESS4545_USERS_PER_CONNECTION: usize = 900;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GamePhase {
    Created,
    Started,
    Finished(GameOutcome),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameObservation {
    pub game: Game,
    pub phase: GamePhase,
}

impl GameObservation {
    #[must_use]
    pub fn into_event(self) -> Option<GameEvent> {
        match self.phase {
            GamePhase::Created => None,
            GamePhase::Started => Some(GameEvent::Started(self.game)),
            GamePhase::Finished(outcome) => Some(GameEvent::Finished {
                game: self.game,
                outcome,
            }),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FeedEvent {
    Heartbeat,
    Observed(GameObservation),
}

#[derive(Debug, thiserror::Error)]
pub enum FeedError {
    #[error("rate limited by lichess")]
    RateLimited,
    #[error("lichess sent nothing for {0:?}")]
    Inactive(Duration),
    #[error("lichess is unavailable")]
    Unavailable(#[source] Box<dyn std::error::Error + Send + Sync>),
}

#[derive(Debug, thiserror::Error)]
enum GameStreamError {
    #[error("lichess answered with status {0}")]
    UnexpectedStatus(StatusCode),
    #[error("lichess request failed")]
    Request(#[from] reqwest::Error),
    #[error("lichess stream broke")]
    Lines(#[from] LinesCodecError),
}

impl From<GameStreamError> for FeedError {
    fn from(error: GameStreamError) -> Self {
        Self::Unavailable(Box::new(error))
    }
}

#[derive(Clone, Debug)]
pub struct LichessStream {
    http: reqwest::Client,
    endpoint: Url,
    token: Option<SecretString>,
    inactivity_timeout: Duration,
    users_per_connection: usize,
}

impl LichessStream {
    pub fn new(http: reqwest::Client, settings: &LichessSettings) -> Result<Self, url::ParseError> {
        Ok(Self {
            http,
            endpoint: settings.base_url.join(GAMES_BY_USERS_PATH)?,
            token: settings.token.clone(),
            inactivity_timeout: settings.stream.inactivity_timeout,
            users_per_connection: if settings.token.is_some() {
                LICHESS4545_USERS_PER_CONNECTION
            } else {
                ANONYMOUS_USERS_PER_CONNECTION
            },
        })
    }

    #[must_use]
    pub const fn users_per_connection(&self) -> usize {
        self.users_per_connection
    }

    async fn open(&self, user_ids: &[UserId]) -> Result<reqwest::Response, FeedError> {
        let body = user_ids
            .iter()
            .map(|user_id| user_id.as_str())
            .collect::<Vec<_>>()
            .join(",");
        let mut request = self
            .http
            .post(self.endpoint.clone())
            .header(CONTENT_TYPE, "text/plain")
            .body(body);
        if let Some(token) = &self.token {
            request = request.bearer_auth(token.expose_secret());
        }
        tokio::time::timeout(self.inactivity_timeout, request.send())
            .await
            .map_err(|_| FeedError::Inactive(self.inactivity_timeout))?
            .map_err(|error| GameStreamError::Request(error).into())
    }

    pub async fn connect(&self, user_ids: &[UserId]) -> Result<LichessFeed, FeedError> {
        let response = self.open(user_ids).await?;
        match response.status() {
            StatusCode::OK => {}
            StatusCode::TOO_MANY_REQUESTS => return Err(FeedError::RateLimited),
            status => return Err(GameStreamError::UnexpectedStatus(status).into()),
        }
        let bytes = response.bytes_stream().map_err(std::io::Error::other);
        let lines = FramedRead::new(
            StreamReader::new(bytes),
            LinesCodec::new_with_max_length(MAX_LINE_BYTES),
        );
        Ok(LichessFeed {
            lines: Box::pin(lines),
            inactivity_timeout: self.inactivity_timeout,
        })
    }
}

type Lines = Pin<Box<dyn Stream<Item = Result<String, LinesCodecError>> + Send>>;

pub struct LichessFeed {
    lines: Lines,
    inactivity_timeout: Duration,
}

impl fmt::Debug for LichessFeed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LichessFeed")
            .field("inactivity_timeout", &self.inactivity_timeout)
            .finish_non_exhaustive()
    }
}

impl LichessFeed {
    pub async fn next_event(&mut self) -> Option<Result<FeedEvent, FeedError>> {
        loop {
            let line = match tokio::time::timeout(self.inactivity_timeout, self.lines.next()).await
            {
                Err(_) => return Some(Err(FeedError::Inactive(self.inactivity_timeout))),
                Ok(None) => return None,
                Ok(Some(Err(LinesCodecError::MaxLineLengthExceeded))) => {
                    tracing::warn!(
                        max_bytes = MAX_LINE_BYTES,
                        "skipping oversized lichess line"
                    );
                    continue;
                }
                Ok(Some(Err(error))) => return Some(Err(GameStreamError::Lines(error).into())),
                Ok(Some(Ok(line))) => line,
            };
            if line.trim().is_empty() {
                return Some(Ok(FeedEvent::Heartbeat));
            }
            match json::decode(&line) {
                Ok(observation) => return Some(Ok(FeedEvent::Observed(observation))),
                Err(error) => {
                    tracing::warn!(error = %error, line = %line, "skipping undecodable lichess game");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
