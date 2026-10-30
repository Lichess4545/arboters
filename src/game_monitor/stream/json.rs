use serde::Deserialize;

use crate::game_monitor::{Ending, GameOutcome};
use crate::lichess::{Clock, Color, Game, GameId, Player, Speed, UserId, Variant};

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
    variant: Variant,
    speed: Speed,
    clock: Option<Clock>,
    status: u16,
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

const fn phase(status: u16, winner: Option<Color>) -> Result<GamePhase, GameJsonError> {
    let ending = match status {
        10 => return Ok(GamePhase::Created),
        20 => return Ok(GamePhase::Started),
        25 => Ending::Aborted,
        30 => Ending::Checkmate,
        31 => Ending::Resignation,
        32 => Ending::Stalemate,
        33 => Ending::VictoryClaimed,
        34 => Ending::Draw,
        35 => Ending::OutOfTime,
        36 => Ending::CheatDetected,
        37 => Ending::NeverStarted,
        38 => Ending::Unknown,
        39 => Ending::InsufficientMaterialClaimed,
        60 => Ending::VariantEnd,
        unknown => return Err(GameJsonError::UnknownStatusCode(unknown)),
    };
    Ok(GamePhase::Finished {
        outcome: outcome(ending, winner),
        ending,
    })
}

const fn outcome(ending: Ending, winner: Option<Color>) -> GameOutcome {
    match (ending, winner) {
        (Ending::Aborted | Ending::NeverStarted, _) => GameOutcome::Aborted,
        (Ending::Draw | Ending::Stalemate | Ending::InsufficientMaterialClaimed, _) => {
            GameOutcome::Draw
        }
        (_, Some(Color::White)) => GameOutcome::WhiteWon,
        (_, Some(Color::Black)) => GameOutcome::BlackWon,
        (Ending::Unknown, None) => GameOutcome::Undecided,
        (_, None) => GameOutcome::Draw,
    }
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
    Ok(GameObservation {
        phase: phase(game.status, game.winner)?,
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

#[cfg(test)]
mod tests;
