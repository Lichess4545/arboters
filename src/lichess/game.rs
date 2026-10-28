use nutype::nutype;
use serde::Deserialize;

use super::UserId;

/// ```
/// use arboters::lichess::GameId;
///
/// assert_eq!(GameId::new("abcd1234").link(), "https://lichess.org/abcd1234");
/// ```
#[nutype(derive(Debug, Clone, PartialEq, Eq, Hash, Deref, Display, Deserialize))]
pub struct GameId(String);

impl GameId {
    #[must_use]
    pub fn link(&self) -> String {
        format!("https://lichess.org/{self}")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Color {
    White,
    Black,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Player {
    pub user_id: UserId,
    pub rating: Option<u16>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Speed {
    UltraBullet,
    Bullet,
    Blitz,
    Rapid,
    Classical,
    Correspondence,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Game {
    pub id: GameId,
    pub white: Player,
    pub black: Player,
    pub speed: Speed,
    pub rated: bool,
}

impl Game {
    #[must_use]
    pub const fn user_ids(&self) -> [&UserId; 2] {
        [&self.white.user_id, &self.black.user_id]
    }
}

#[cfg(test)]
mod tests;
