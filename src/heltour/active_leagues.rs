use std::collections::BTreeSet;

use crate::lichess;

use super::league::{League, Tag};
use super::pairing::Pairing;
use super::team_pairing::{Board, TeamPairing};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ActiveLeagues {
    pub leagues: Vec<League>,
    pub pairings: Vec<Pairing>,
    pub team_pairings: Vec<TeamPairing>,
}

impl ActiveLeagues {
    #[must_use]
    pub fn leagues_of(&self, game: &lichess::Game) -> BTreeSet<&Tag> {
        self.pairings
            .iter()
            .filter(|pairing| pairing.pairs_the_players_of(game))
            .map(|pairing| &pairing.league)
            .collect()
    }

    #[must_use]
    pub fn team_board_of(&self, game: &lichess::Game) -> Option<(&TeamPairing, &Board)> {
        let leagues = self.leagues_of(game);
        let mut seated = self
            .team_pairings
            .iter()
            .filter(|team_pairing| leagues.contains(&team_pairing.league))
            .flat_map(|team_pairing| {
                team_pairing
                    .boards
                    .iter()
                    .filter(|board| board.seats_the_players_of(game))
                    .map(move |board| (team_pairing, board))
            });
        match (seated.next(), seated.next()) {
            (Some(only), None) => Some(only),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;
