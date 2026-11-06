use crate::lichess::{Color, GameStatus};

use super::event::{Ending, GameOutcome};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GamePhase {
    Created,
    Started,
    Finished {
        outcome: GameOutcome,
        ending: Ending,
    },
}

impl GamePhase {
    #[must_use]
    pub const fn of(status: GameStatus) -> Self {
        match status {
            GameStatus::Created => Self::Created,
            GameStatus::Started => Self::Started,
            GameStatus::Aborted => Self::finished(Ending::Aborted, None),
            GameStatus::Mate { winner } => Self::finished(Ending::Checkmate, Some(winner)),
            GameStatus::Resign { winner } => Self::finished(Ending::Resignation, Some(winner)),
            GameStatus::Stalemate => Self::finished(Ending::Stalemate, None),
            GameStatus::Timeout { winner } => Self::finished(Ending::VictoryClaimed, winner),
            GameStatus::Draw => Self::finished(Ending::Draw, None),
            GameStatus::Outoftime { winner } => Self::finished(Ending::OutOfTime, winner),
            GameStatus::Cheat { winner } => Self::finished(Ending::CheatDetected, Some(winner)),
            GameStatus::NoStart { winner } => Self::finished(Ending::NeverStarted, Some(winner)),
            GameStatus::UnknownFinish { winner } => Self::finished(Ending::Unknown, winner),
            GameStatus::InsufficientMaterialClaim => {
                Self::finished(Ending::InsufficientMaterialClaimed, None)
            }
            GameStatus::VariantEnd { winner } => Self::finished(Ending::VariantEnd, Some(winner)),
        }
    }

    const fn finished(ending: Ending, winner: Option<Color>) -> Self {
        Self::Finished {
            outcome: outcome(ending, winner),
            ending,
        }
    }
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

#[cfg(test)]
mod tests;
