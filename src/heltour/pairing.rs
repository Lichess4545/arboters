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
    pub(super) fn pairs_the_players_of(&self, game: &lichess::Game) -> bool {
        let players = game.user_ids();
        players == [&self.white, &self.black] || players == [&self.black, &self.white]
    }
}
