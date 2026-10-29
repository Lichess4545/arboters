use crate::game_monitor::{GameEvent, GameOutcome};
use crate::heltour::TeamPairing;
use crate::heltour::team;
use crate::heltour::team_pairing::{Board, BoardNumber, HalfPoints};
use crate::lichess;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    White,
    Black,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Score {
    pub white: HalfPoints,
    pub black: HalfPoints,
}

impl Score {
    fn plus(self, white: HalfPoints, black: HalfPoints) -> Self {
        Self {
            white: self.white + white,
            black: self.black + black,
        }
    }

    #[must_use]
    pub fn leader(self) -> Option<Side> {
        match self.white.cmp(&self.black) {
            std::cmp::Ordering::Greater => Some(Side::White),
            std::cmp::Ordering::Less => Some(Side::Black),
            std::cmp::Ordering::Equal => None,
        }
    }

    const fn lead(self) -> u16 {
        self.white.into_inner().abs_diff(self.black.into_inner())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchResult {
    Won(Side),
    Drawn,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Standing {
    pub score: Score,
    pub unfinished_boards: u16,
}

const ONE_BOARD_RESULTS: [(HalfPoints, HalfPoints); 3] = [
    (HalfPoints::ONE_POINT, HalfPoints::ZERO),
    (HalfPoints::HALF_POINT, HalfPoints::HALF_POINT),
    (HalfPoints::ZERO, HalfPoints::ONE_POINT),
];

impl Standing {
    #[must_use]
    pub fn settled(self) -> Option<MatchResult> {
        let still_open = self
            .unfinished_boards
            .saturating_mul(HalfPoints::ONE_POINT.into_inner());
        if self.unfinished_boards > 0 && self.score.lead() <= still_open {
            return None;
        }
        Some(
            self.score
                .leader()
                .map_or(MatchResult::Drawn, MatchResult::Won),
        )
    }

    #[must_use]
    pub fn one_game_can_settle(self) -> bool {
        let Some(after_one) = self.unfinished_boards.checked_sub(1) else {
            return false;
        };
        self.settled().is_none()
            && ONE_BOARD_RESULTS.iter().any(|&(white, black)| {
                Self {
                    score: self.score.plus(white, black),
                    unfinished_boards: after_one,
                }
                .settled()
                .is_some()
            })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TeamMatch<'a> {
    pub white_team: &'a team::DisplayName,
    pub black_team: &'a team::DisplayName,
    pub standing: Standing,
    pub boards_in_play: Vec<BoardNumber>,
    pub this_game_can_decide: bool,
}

impl<'a> TeamMatch<'a> {
    #[must_use]
    pub fn of(event: &GameEvent, pairing: &'a TeamPairing, board: &'a Board) -> Option<Self> {
        let heltour_score = Score {
            white: pairing.white_points,
            black: pairing.black_points,
        };
        let unfinished_boards = pairing
            .boards
            .iter()
            .filter(|board| board.result.is_none())
            .count();
        let heltour_standing = Standing {
            score: heltour_score,
            unfinished_boards: u16::try_from(unfinished_boards).ok()?,
        };
        let (standing, this_game_can_decide) = match event {
            GameEvent::Started(_) => (
                heltour_standing,
                board.result.is_none() && heltour_standing.one_game_can_settle(),
            ),
            GameEvent::Finished { game, outcome, .. } => {
                let (white, black) = team_points(pairing, board, game, *outcome)?;
                let standing = if board.result.is_some() {
                    heltour_standing
                } else {
                    Standing {
                        score: heltour_score.plus(white, black),
                        unfinished_boards: heltour_standing.unfinished_boards.saturating_sub(1),
                    }
                };
                (standing, false)
            }
        };
        let mut boards_in_play: Vec<_> = pairing
            .boards
            .iter()
            .filter(|other| other.number != board.number && other.is_in_play())
            .map(|other| other.number)
            .collect();
        boards_in_play.sort_unstable();
        Some(Self {
            white_team: &pairing.white_team,
            black_team: &pairing.black_team,
            standing,
            boards_in_play,
            this_game_can_decide,
        })
    }
}

fn team_points(
    pairing: &TeamPairing,
    board: &Board,
    game: &lichess::Game,
    outcome: GameOutcome,
) -> Option<(HalfPoints, HalfPoints)> {
    let (game_white, game_black) = match outcome {
        GameOutcome::WhiteWon => (HalfPoints::ONE_POINT, HalfPoints::ZERO),
        GameOutcome::Draw => (HalfPoints::HALF_POINT, HalfPoints::HALF_POINT),
        GameOutcome::BlackWon => (HalfPoints::ZERO, HalfPoints::ONE_POINT),
        GameOutcome::Aborted | GameOutcome::Undecided => return None,
    };
    let game_white_team = if board.white.as_ref() == Some(&game.white.user_id) {
        &board.white_team
    } else {
        &board.black_team
    };
    if *game_white_team == pairing.white_team {
        Some((game_white, game_black))
    } else if *game_white_team == pairing.black_team {
        Some((game_black, game_white))
    } else {
        None
    }
}

#[cfg(test)]
mod tests;
