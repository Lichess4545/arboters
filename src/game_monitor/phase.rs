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
    pub const fn of(status: GameStatus, winner: Option<Color>) -> Self {
        match status {
            GameStatus::Created => Self::Created,
            GameStatus::Started => Self::Started,
            GameStatus::Aborted => Self::finished(Ending::Aborted, winner),
            GameStatus::Mate => Self::finished(Ending::Checkmate, winner),
            GameStatus::Resign => Self::finished(Ending::Resignation, winner),
            GameStatus::Stalemate => Self::finished(Ending::Stalemate, winner),
            GameStatus::Timeout => Self::finished(Ending::VictoryClaimed, winner),
            GameStatus::Draw => Self::finished(Ending::Draw, winner),
            GameStatus::Outoftime => Self::finished(Ending::OutOfTime, winner),
            GameStatus::Cheat => Self::finished(Ending::CheatDetected, winner),
            GameStatus::NoStart => Self::finished(Ending::NeverStarted, winner),
            GameStatus::UnknownFinish => Self::finished(Ending::Unknown, winner),
            GameStatus::InsufficientMaterialClaim => {
                Self::finished(Ending::InsufficientMaterialClaimed, winner)
            }
            GameStatus::VariantEnd => Self::finished(Ending::VariantEnd, winner),
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
