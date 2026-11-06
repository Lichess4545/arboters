use crate::lichess::{Game, GameState};

use super::phase::GamePhase;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GameOutcome {
    WhiteWon,
    BlackWon,
    Draw,
    Aborted,
    Undecided,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Ending {
    Checkmate,
    Resignation,
    Stalemate,
    Draw,
    OutOfTime,
    InsufficientMaterialClaimed,
    VariantEnd,
    VictoryClaimed,
    CheatDetected,
    Aborted,
    NeverStarted,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GameEventKind {
    GameStarted,
    GameFinished,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GameEvent {
    Started(Game),
    Finished {
        game: Game,
        outcome: GameOutcome,
        ending: Ending,
    },
}

impl GameEvent {
    #[must_use]
    pub fn of(state: GameState) -> Option<Self> {
        match GamePhase::of(state.status) {
            GamePhase::Created => None,
            GamePhase::Started => Some(Self::Started(state.game)),
            GamePhase::Finished { outcome, ending } => Some(Self::Finished {
                game: state.game,
                outcome,
                ending,
            }),
        }
    }

    #[must_use]
    pub const fn game(&self) -> &Game {
        match self {
            Self::Started(game) | Self::Finished { game, .. } => game,
        }
    }

    #[must_use]
    pub const fn kind(&self) -> GameEventKind {
        match self {
            Self::Started(_) => GameEventKind::GameStarted,
            Self::Finished { .. } => GameEventKind::GameFinished,
        }
    }
}
