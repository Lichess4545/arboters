use std::collections::BTreeSet;

use crate::lichess;

use super::league::{League, Tag};
use super::pairing::Pairing;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ActiveLeagues {
    pub leagues: Vec<League>,
    pub pairings: Vec<Pairing>,
}

impl ActiveLeagues {
    #[must_use]
    pub fn includes(&self, tag: &Tag) -> bool {
        self.leagues.iter().any(|league| &league.tag == tag)
    }

    #[must_use]
    pub fn leagues_of(&self, game: &lichess::Game) -> BTreeSet<&Tag> {
        self.pairings
            .iter()
            .filter(|pairing| pairing.pairs_the_players_of(game))
            .map(|pairing| &pairing.league)
            .collect()
    }
}
