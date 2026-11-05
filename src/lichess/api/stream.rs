use std::fmt;
use std::pin::Pin;
use std::time::Duration;

use futures::{Stream, StreamExt, TryStreamExt};
use reqwest::StatusCode;
use reqwest::header::CONTENT_TYPE;
use serde::Deserialize;
use tokio_util::codec::{FramedRead, LinesCodec, LinesCodecError};
use tokio_util::io::StreamReader;

use crate::lichess::{
    Clock, Color, Game, GameId, GameState, GameStatus, Player, Speed, UserId, Variant,
};

use super::Api;

const GAMES_BY_USERS_PATH: &str = "api/stream/games-by-users";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FeedEvent {
    Heartbeat,
    Game(GameState),
}

#[derive(Debug, thiserror::Error)]
pub enum FeedError {
    #[error("rate limited by lichess")]
    RateLimited,
    #[error("lichess sent nothing for {0:?}")]
    Inactive(Duration),
    #[error("lichess is unavailable")]
    Unavailable(#[source] Box<dyn std::error::Error + Send + Sync>),
    #[error("could not read a lichess game: {line}")]
    Undecodable {
        line: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("lichess sent a line longer than {max_bytes} bytes")]
    LineTooLong { max_bytes: usize },
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

impl Api {
    pub async fn stream_games_by_users(
        &self,
        user_ids: &[UserId],
        inactivity_timeout: Duration,
        max_line_bytes: usize,
    ) -> Result<GameFeed, FeedError> {
        let response = tokio::time::timeout(
            inactivity_timeout,
            self.stream_games_by_users_request(user_ids).send(),
        )
        .await
        .map_err(|_| FeedError::Inactive(inactivity_timeout))?
        .map_err(GameStreamError::Request)?;
        match response.status() {
            StatusCode::OK => Ok(GameFeed::reading(
                response,
                inactivity_timeout,
                max_line_bytes,
            )),
            StatusCode::TOO_MANY_REQUESTS => Err(FeedError::RateLimited),
            status => Err(GameStreamError::UnexpectedStatus(status).into()),
        }
    }

    #[expect(clippy::expect_used)]
    fn stream_games_by_users_request(&self, user_ids: &[UserId]) -> reqwest::RequestBuilder {
        let body = user_ids
            .iter()
            .map(|user_id| user_id.as_str())
            .collect::<Vec<_>>()
            .join(",");
        self.authorized(
            self.http
                .post(self.site.join(GAMES_BY_USERS_PATH).expect("constant path"))
                .header(CONTENT_TYPE, "text/plain")
                .body(body),
        )
    }
}

type LinesStream = Pin<Box<dyn Stream<Item = Result<String, LinesCodecError>> + Send>>;

pub struct GameFeed {
    lines: LinesStream,
    inactivity_timeout: Duration,
    max_line_bytes: usize,
}

impl fmt::Debug for GameFeed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GameFeed")
            .field("inactivity_timeout", &self.inactivity_timeout)
            .finish_non_exhaustive()
    }
}

impl GameFeed {
    fn reading(
        response: reqwest::Response,
        inactivity_timeout: Duration,
        max_line_bytes: usize,
    ) -> Self {
        let bytes = response.bytes_stream().map_err(std::io::Error::other);
        let lines = FramedRead::new(
            StreamReader::new(bytes),
            LinesCodec::new_with_max_length(max_line_bytes),
        );
        Self {
            lines: Box::pin(lines),
            inactivity_timeout,
            max_line_bytes,
        }
    }

    pub async fn next_event(&mut self) -> Option<Result<FeedEvent, FeedError>> {
        let line = match tokio::time::timeout(self.inactivity_timeout, self.lines.next()).await {
            Err(_) => return Some(Err(FeedError::Inactive(self.inactivity_timeout))),
            Ok(None) => return None,
            Ok(Some(Err(LinesCodecError::MaxLineLengthExceeded))) => {
                return Some(Err(FeedError::LineTooLong {
                    max_bytes: self.max_line_bytes,
                }));
            }
            Ok(Some(Err(error))) => return Some(Err(GameStreamError::Lines(error).into())),
            Ok(Some(Ok(line))) => line,
        };
        if line.trim().is_empty() {
            return Some(Ok(FeedEvent::Heartbeat));
        }
        Some(
            decode(&line)
                .map(FeedEvent::Game)
                .map_err(|source| FeedError::Undecodable { line, source }),
        )
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StreamedGame {
    id: GameId,
    rated: bool,
    variant: Variant,
    speed: Speed,
    clock: Option<Clock>,
    status_name: GameStatus,
    players: StreamedPlayers,
    winner: Option<Color>,
}

#[derive(Debug, Deserialize)]
struct StreamedPlayers {
    white: StreamedPlayer,
    black: StreamedPlayer,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StreamedPlayer {
    user_id: UserId,
    // The games-by-users stream spec types rating as an integer:
    // https://github.com/lichess-org/api/blob/c1644a780141566a68b7dce113151d9cc319b004/doc/specs/schemas/GameStreamGame.yaml#L36-L37
    rating: Option<u16>,
}

impl From<StreamedPlayer> for Player {
    fn from(player: StreamedPlayer) -> Self {
        Self {
            user_id: player.user_id,
            rating: player.rating,
        }
    }
}

fn decode(line: &str) -> Result<GameState, serde_json::Error> {
    let game: StreamedGame = serde_json::from_str(line)?;
    Ok(GameState {
        status: game.status_name,
        winner: game.winner,
        game: Game {
            id: game.id,
            white: game.players.white.into(),
            black: game.players.black.into(),
            speed: game.speed,
            rated: game.rated,
            variant: game.variant,
            clock: game.clock,
        },
    })
}
