use nutype::nutype;
use serde::Deserialize;
use url::Url;

use super::UserId;

/// ```
/// use arboters::lichess::GameId;
/// use url::Url;
///
/// let lichess = Url::parse("https://lichess.org").unwrap();
/// assert_eq!(GameId::new("abcd1234").link(&lichess).as_str(), "https://lichess.org/game/abcd1234");
///
/// let local = Url::parse("http://localhost:9663/").unwrap();
/// assert_eq!(GameId::new("abcd1234").link(&local).as_str(), "http://localhost:9663/game/abcd1234");
/// ```
#[nutype(derive(Debug, Clone, PartialEq, Eq, Hash, Deref, Display, Deserialize))]
pub struct GameId(String);

impl GameId {
    // Lichess moved game pages from /<id> to /game/<id> in October 2026; bare links now 301 there.
    // https://github.com/lichess-org/lila/commit/3c70aa350c3a06a7522f43f95db65d8445d89ca7
    #[must_use]
    pub fn link(&self, lichess: &Url) -> Url {
        let mut link = lichess.clone();
        link.set_path(&format!(
            "{}/game/{self}",
            lichess.path().trim_end_matches('/')
        ));
        link
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
    // Lichess game ratings are integers (lila `rating: Option[IntRating]`, an opaque Int):
    // https://github.com/lichess-org/lila/blob/898ad2cdee0aa803d7959d83868efc00097a648b/modules/core/src/main/game/Player.scala#L21
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Variant {
    Standard,
    Chess960,
    Crazyhouse,
    Antichess,
    Atomic,
    Horde,
    KingOfTheHill,
    RacingKings,
    ThreeCheck,
    FromPosition,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize)]
pub struct Clock {
    #[serde(rename = "initial")]
    pub initial_seconds: u32,
    #[serde(rename = "increment")]
    pub increment_seconds: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Game {
    pub id: GameId,
    pub white: Player,
    pub black: Player,
    pub speed: Speed,
    pub rated: bool,
    pub variant: Variant,
    pub clock: Option<Clock>,
}

impl Game {
    #[must_use]
    pub const fn user_ids(&self) -> [&UserId; 2] {
        [&self.white.user_id, &self.black.user_id]
    }
}
