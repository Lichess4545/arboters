use std::time::Duration;

use reqwest::StatusCode;
use reqwest::header::{ACCEPT, HeaderValue};
use serde::Deserialize;
use time::OffsetDateTime;

use crate::lichess::game::{ImpossibleStatus, StatusName};
use crate::lichess::{
    Clock, Color, Game, GameId, GameState, GameStatus, Player, Speed, UserId, Variant,
};

use super::Api;

const GAMES_USER_PATH: &str = "api/games/user/";
const NDJSON: HeaderValue = HeaderValue::from_static("application/x-ndjson");

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GamesUserQuery {
    pub vs: Option<UserId>,
    pub since: Option<OffsetDateTime>,
    pub finished: Option<bool>,
    pub ongoing: Option<bool>,
}

impl GamesUserQuery {
    fn params(&self) -> Vec<(&'static str, String)> {
        let mut params = vec![("moves", false.to_string())];
        if let Some(vs) = &self.vs {
            params.push(("vs", vs.to_string()));
        }
        if let Some(since) = self.since {
            params.push(("since", epoch_millis(since).to_string()));
        }
        if let Some(finished) = self.finished {
            params.push(("finished", finished.to_string()));
        }
        if let Some(ongoing) = self.ongoing {
            params.push(("ongoing", ongoing.to_string()));
        }
        params
    }
}

const fn epoch_millis(at: OffsetDateTime) -> i128 {
    at.unix_timestamp_nanos() / 1_000_000
}

#[derive(Debug, thiserror::Error)]
pub enum GamesUserError {
    #[error("rate limited by lichess")]
    RateLimited,
    #[error("lichess answered with status {0}")]
    UnexpectedStatus(StatusCode),
    #[error("lichess request failed")]
    Request(#[from] reqwest::Error),
    #[error("could not read a lichess game: {line}")]
    Undecodable {
        line: String,
        #[source]
        source: serde_json::Error,
    },
}

impl Api {
    pub async fn games_user(
        &self,
        username: &UserId,
        query: &GamesUserQuery,
        timeout: Duration,
    ) -> Result<Vec<GameState>, GamesUserError> {
        let response = self
            .games_user_request(username, query)
            .timeout(timeout)
            .send()
            .await?;
        match response.status() {
            StatusCode::OK => decode_lines(&response.text().await?),
            StatusCode::TOO_MANY_REQUESTS => Err(GamesUserError::RateLimited),
            status => Err(GamesUserError::UnexpectedStatus(status)),
        }
    }

    #[expect(clippy::expect_used)]
    fn games_user_request(
        &self,
        username: &UserId,
        query: &GamesUserQuery,
    ) -> reqwest::RequestBuilder {
        let path = format!("{GAMES_USER_PATH}{username}");
        self.authorized(
            self.http
                .get(self.site.join(&path).expect("relative path of a user id"))
                .header(ACCEPT, NDJSON)
                .query(&query.params()),
        )
    }
}

#[derive(Debug, Deserialize)]
struct ExportedGame {
    id: GameId,
    rated: bool,
    variant: Variant,
    speed: Speed,
    clock: Option<Clock>,
    status: StatusName,
    players: ExportedPlayers,
    winner: Option<Color>,
}

#[derive(Debug, Deserialize)]
struct ExportedPlayers {
    white: ExportedPlayer,
    black: ExportedPlayer,
}

#[derive(Debug, Deserialize)]
struct ExportedPlayer {
    user: ExportedUser,
    rating: Option<u16>,
}

#[derive(Debug, Deserialize)]
struct ExportedUser {
    id: UserId,
}

impl From<ExportedPlayer> for Player {
    fn from(player: ExportedPlayer) -> Self {
        Self {
            user_id: player.user.id,
            rating: player.rating,
        }
    }
}

impl TryFrom<ExportedGame> for GameState {
    type Error = ImpossibleStatus;

    fn try_from(game: ExportedGame) -> Result<Self, Self::Error> {
        Ok(Self {
            status: GameStatus::new(game.status, game.winner)?,
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
}

fn decode_lines(body: &str) -> Result<Vec<GameState>, GamesUserError> {
    body.lines()
        .filter(|line| !line.trim().is_empty())
        .map(decode)
        .collect()
}

fn decode(line: &str) -> Result<GameState, GamesUserError> {
    serde_json::from_str::<ExportedGame>(line)
        .and_then(|game| GameState::try_from(game).map_err(serde::de::Error::custom))
        .map_err(|source| GamesUserError::Undecodable {
            line: line.to_owned(),
            source,
        })
}
