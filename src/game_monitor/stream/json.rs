use serde::Deserialize;

use crate::game_monitor::GameOutcome;
use crate::lichess::{Color, Game, GameId, Player, Speed, UserId};

use super::{GameObservation, GamePhase};

#[derive(Debug, thiserror::Error)]
pub enum GameJsonError {
    #[error("malformed game json")]
    Json(#[from] serde_json::Error),
    #[error("unknown lichess game status code {0}")]
    UnknownStatusCode(u16),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GameJson {
    id: GameId,
    rated: bool,
    speed: Speed,
    status: StatusJson,
    players: PlayersJson,
    winner: Option<Color>,
}

#[derive(Debug, Deserialize)]
struct PlayersJson {
    white: PlayerJson,
    black: PlayerJson,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlayerJson {
    user_id: UserId,
    // The games-by-users stream spec types rating as an integer:
    // https://github.com/lichess-org/api/blob/c1644a780141566a68b7dce113151d9cc319b004/doc/specs/schemas/GameStreamGame.yaml#L36-L37
    rating: Option<u16>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum StatusJson {
    Code(u16),
    Name(GameStatus),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
enum GameStatus {
    Created,
    Started,
    Aborted,
    Mate,
    Resign,
    Stalemate,
    Timeout,
    Draw,
    #[serde(alias = "outOfTime")]
    Outoftime,
    Cheat,
    #[serde(alias = "nostart")]
    NoStart,
    #[serde(alias = "unknownfinish")]
    UnknownFinish,
    InsufficientMaterialClaim,
    #[serde(alias = "variantend")]
    VariantEnd,
}

impl TryFrom<StatusJson> for GameStatus {
    type Error = GameJsonError;

    fn try_from(status: StatusJson) -> Result<Self, Self::Error> {
        let code = match status {
            StatusJson::Name(name) => return Ok(name),
            StatusJson::Code(code) => code,
        };
        Ok(match code {
            10 => Self::Created,
            20 => Self::Started,
            25 => Self::Aborted,
            30 => Self::Mate,
            31 => Self::Resign,
            32 => Self::Stalemate,
            33 => Self::Timeout,
            34 => Self::Draw,
            35 => Self::Outoftime,
            36 => Self::Cheat,
            37 => Self::NoStart,
            38 => Self::UnknownFinish,
            39 => Self::InsufficientMaterialClaim,
            60 => Self::VariantEnd,
            unknown => return Err(GameJsonError::UnknownStatusCode(unknown)),
        })
    }
}

const fn phase(status: GameStatus, winner: Option<Color>) -> GamePhase {
    let outcome = match (status, winner) {
        (GameStatus::Created, _) => return GamePhase::Created,
        (GameStatus::Started, _) => return GamePhase::Started,
        (GameStatus::Aborted | GameStatus::NoStart, _) => GameOutcome::Aborted,
        (GameStatus::Draw | GameStatus::Stalemate | GameStatus::InsufficientMaterialClaim, _) => {
            GameOutcome::Draw
        }
        (_, Some(Color::White)) => GameOutcome::WhiteWon,
        (_, Some(Color::Black)) => GameOutcome::BlackWon,
        (GameStatus::UnknownFinish, None) => GameOutcome::Undecided,
        (_, None) => GameOutcome::Draw,
    };
    GamePhase::Finished(outcome)
}

impl From<PlayerJson> for Player {
    fn from(player: PlayerJson) -> Self {
        Self {
            user_id: player.user_id,
            rating: player.rating,
        }
    }
}

pub fn decode(line: &str) -> Result<GameObservation, GameJsonError> {
    let game: GameJson = serde_json::from_str(line)?;
    let status = GameStatus::try_from(game.status)?;
    Ok(GameObservation {
        phase: phase(status, game.winner),
        game: Game {
            id: game.id,
            white: game.players.white.into(),
            black: game.players.black.into(),
            speed: game.speed,
            rated: game.rated,
        },
    })
}

#[cfg(test)]
mod tests;
