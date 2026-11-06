use std::collections::BTreeSet;

use crate::lichess::{Color, Game, GameState, GameStatus};

use super::League;
use super::fixtures::{Fixture, FixturesId};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Decision {
    WhiteWon(Win),
    BlackWon(Win),
    Drawn(Draw),
}

impl Decision {
    const fn of(status: GameStatus) -> Option<Self> {
        let decision = match status {
            GameStatus::Created
            | GameStatus::Started
            | GameStatus::Aborted
            | GameStatus::NoStart { .. } => return None,
            GameStatus::Draw => Self::Drawn(Draw::AgreementOrRule),
            GameStatus::Stalemate => Self::Drawn(Draw::Stalemate),
            GameStatus::InsufficientMaterialClaim => Self::Drawn(Draw::InsufficientMaterialClaimed),
            GameStatus::Mate { winner } => Self::won(winner, Win::Checkmate),
            GameStatus::Resign { winner } => Self::won(winner, Win::Resignation),
            GameStatus::Cheat { winner } => Self::won(winner, Win::CheatDetected),
            GameStatus::VariantEnd { winner } => Self::won(winner, Win::VariantEnd),
            GameStatus::Outoftime { winner } => {
                Self::won_or_drawn(winner, Win::OutOfTime, Draw::OutOfTime)
            }
            GameStatus::Timeout { winner } => {
                Self::won_or_drawn(winner, Win::VictoryClaimed, Draw::PlayerLeft)
            }
            GameStatus::UnknownFinish { winner } => {
                Self::won_or_drawn(winner, Win::Unknown, Draw::Unknown)
            }
        };
        Some(decision)
    }

    const fn won(winner: Color, win: Win) -> Self {
        match winner {
            Color::White => Self::WhiteWon(win),
            Color::Black => Self::BlackWon(win),
        }
    }

    const fn won_or_drawn(winner: Option<Color>, win: Win, draw: Draw) -> Self {
        match winner {
            Some(winner) => Self::won(winner, win),
            None => Self::Drawn(draw),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Win {
    Checkmate,
    Resignation,
    OutOfTime,
    VariantEnd,
    VictoryClaimed,
    CheatDetected,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Draw {
    AgreementOrRule,
    Stalemate,
    InsufficientMaterialClaimed,
    OutOfTime,
    PlayerLeft,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VoidReason {
    Aborted,
    NeverStarted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GameEventKind {
    GameStarted,
    GameFinished,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GameChange {
    Started(Game),
    Decided { game: Game, decision: Decision },
    Void { game: Game, reason: VoidReason },
}

impl GameChange {
    #[must_use]
    pub fn of(state: GameState) -> Option<Self> {
        let game = state.game;
        match state.status {
            GameStatus::Created => None,
            GameStatus::Started => Some(Self::Started(game)),
            GameStatus::Aborted => Some(Self::Void {
                game,
                reason: VoidReason::Aborted,
            }),
            GameStatus::NoStart { .. } => Some(Self::Void {
                game,
                reason: VoidReason::NeverStarted,
            }),
            finished => Decision::of(finished).map(|decision| Self::Decided { game, decision }),
        }
    }

    #[must_use]
    pub const fn game(&self) -> &Game {
        match self {
            Self::Started(game) | Self::Decided { game, .. } | Self::Void { game, .. } => game,
        }
    }

    #[must_use]
    pub const fn kind(&self) -> GameEventKind {
        match self {
            Self::Started(_) => GameEventKind::GameStarted,
            Self::Decided { .. } | Self::Void { .. } => GameEventKind::GameFinished,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchedFixture {
    pub fixtures_id: FixturesId,
    pub league: League,
    pub fixture: Fixture,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameEvent {
    pub change: GameChange,
    pub matched: Vec<MatchedFixture>,
}

impl GameEvent {
    #[must_use]
    pub const fn game(&self) -> &Game {
        self.change.game()
    }

    #[must_use]
    pub const fn kind(&self) -> GameEventKind {
        self.change.kind()
    }

    #[must_use]
    pub fn leagues(&self) -> BTreeSet<&League> {
        self.matched.iter().map(|matched| &matched.league).collect()
    }
}

#[cfg(test)]
mod tests;
