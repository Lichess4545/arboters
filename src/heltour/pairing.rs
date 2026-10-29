use std::collections::BTreeSet;

use serde::Deserialize;

use crate::lichess;

use super::league;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Pairing {
    pub league: league::Tag,
    pub white: lichess::UserId,
    pub black: lichess::UserId,
}

impl Pairing {
    fn pairs_the_players_of(&self, game: &lichess::Game) -> bool {
        let players = game.user_ids();
        players == [&self.white, &self.black] || players == [&self.black, &self.white]
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CurrentPairings(pub Vec<Pairing>);

impl CurrentPairings {
    #[must_use]
    pub fn leagues_of(&self, game: &lichess::Game) -> BTreeSet<&league::Tag> {
        self.0
            .iter()
            .filter(|pairing| pairing.pairs_the_players_of(game))
            .map(|pairing| &pairing.league)
            .collect()
    }
}
