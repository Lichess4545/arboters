use std::fmt;

use serde::Deserialize;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(try_from = "String")]
pub struct LichessUsername(String);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{0}` is not a valid lichess username")]
pub struct InvalidLichessUsername(String);

impl LichessUsername {
    const LENGTH: std::ops::RangeInclusive<usize> = 2..=30;

    pub fn parse(raw: &str) -> Result<Self, InvalidLichessUsername> {
        let candidate = raw.trim();
        let has_valid_length = Self::LENGTH.contains(&candidate.len());
        let has_valid_characters = candidate
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        if has_valid_length && has_valid_characters {
            Ok(Self(candidate.to_ascii_lowercase()))
        } else {
            Err(InvalidLichessUsername(raw.to_owned()))
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for LichessUsername {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for LichessUsername {
    type Error = InvalidLichessUsername;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        Self::parse(&raw)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Deserialize)]
#[serde(transparent)]
pub struct GameId(String);

impl GameId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub fn link(&self) -> String {
        format!("https://lichess.org/{}", self.0)
    }
}

impl fmt::Display for GameId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
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
    pub username: LichessUsername,
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
    pub const fn usernames(&self) -> [&LichessUsername; 2] {
        [&self.white.username, &self.black.username]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GameOutcome {
    WhiteWon,
    BlackWon,
    Draw,
    Aborted,
    Undecided,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GameEventKind {
    GameStarted,
    GameFinished,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GameEvent {
    Started(Game),
    Finished { game: Game, outcome: GameOutcome },
}

impl GameEvent {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usernames_are_normalized_to_lowercase() {
        let username = LichessUsername::parse("  MrScribbles ").unwrap();
        assert_eq!(username.as_str(), "mrscribbles");
    }

    #[test]
    fn usernames_with_invalid_characters_are_rejected() {
        assert!(LichessUsername::parse("not a user").is_err());
        assert!(LichessUsername::parse("x").is_err());
        assert!(LichessUsername::parse(&"a".repeat(31)).is_err());
    }

    #[test]
    fn usernames_deserialize_through_validation() {
        let parsed: LichessUsername = serde_json::from_str("\"Lakin\"").unwrap();
        assert_eq!(parsed.as_str(), "lakin");
        assert!(serde_json::from_str::<LichessUsername>("\"bad name\"").is_err());
    }

    #[test]
    fn speeds_use_the_lichess_spelling() {
        assert_eq!(
            serde_json::from_str::<Speed>("\"ultraBullet\"").unwrap(),
            Speed::UltraBullet
        );
        assert_eq!(
            serde_json::from_str::<Speed>("\"classical\"").unwrap(),
            Speed::Classical
        );
    }

    #[test]
    fn game_links_point_at_lichess() {
        assert_eq!(
            GameId::new("abcd1234").link(),
            "https://lichess.org/abcd1234"
        );
    }
}
