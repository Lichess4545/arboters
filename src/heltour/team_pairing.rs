use nutype::nutype;
use serde::de::{Error, IntoDeserializer};
use serde::{Deserialize, Deserializer};
use url::Url;

use crate::lichess;

use super::{league, team};

/// ```
/// use arboters::heltour::team_pairing::HalfPoints;
///
/// assert_eq!(HalfPoints::from_points(2.5), Some(HalfPoints::new(5)));
/// assert_eq!(HalfPoints::from_points(0.25), None);
/// assert_eq!(HalfPoints::new(3) + HalfPoints::new(2), HalfPoints::new(5));
/// ```
#[nutype(
    const_fn,
    derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)
)]
pub struct HalfPoints(u16);

impl HalfPoints {
    pub const ZERO: Self = Self::new(0);
    pub const HALF_POINT: Self = Self::new(1);
    pub const ONE_POINT: Self = Self::new(2);

    #[must_use]
    pub fn from_points(points: f64) -> Option<Self> {
        let halves = points * 2.0;
        if halves.fract() != 0.0 || !(0.0..=f64::from(u16::MAX)).contains(&halves) {
            return None;
        }
        #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        Some(Self::new(halves as u16))
    }
}

impl std::ops::Add for HalfPoints {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self::new(self.into_inner().saturating_add(other.into_inner()))
    }
}

#[nutype(derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Display,
    Deserialize
))]
pub struct BoardNumber(u16);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum BoardResult {
    #[serde(rename = "1-0")]
    WhiteWon,
    #[serde(rename = "1/2-1/2")]
    Draw,
    #[serde(rename = "0-1")]
    BlackWon,
    #[serde(rename = "1X-0F")]
    WhiteWonByForfeit,
    #[serde(rename = "1/2Z-1/2Z")]
    DrawByForfeit,
    #[serde(rename = "0F-1X")]
    BlackWonByForfeit,
    #[serde(rename = "0F-0F")]
    BothForfeited,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Board {
    #[serde(rename = "board")]
    pub number: BoardNumber,
    pub white: Option<lichess::UserId>,
    pub black: Option<lichess::UserId>,
    pub white_team: team::DisplayName,
    pub black_team: team::DisplayName,
    #[serde(deserialize_with = "unless_blank")]
    pub game_link: Option<Url>,
    #[serde(deserialize_with = "unless_blank")]
    pub result: Option<BoardResult>,
}

impl Board {
    #[must_use]
    pub fn seats_the_players_of(&self, game: &lichess::Game) -> bool {
        let (Some(white), Some(black)) = (&self.white, &self.black) else {
            return false;
        };
        let players = game.user_ids();
        players == [white, black] || players == [black, white]
    }

    #[must_use]
    pub const fn is_in_play(&self) -> bool {
        self.game_link.is_some() && self.result.is_none()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct TeamPairing {
    pub league: league::Tag,
    pub white_team: team::DisplayName,
    #[serde(deserialize_with = "half_points")]
    pub white_points: HalfPoints,
    pub black_team: team::DisplayName,
    #[serde(deserialize_with = "half_points")]
    pub black_points: HalfPoints,
    pub boards: Vec<Board>,
}

fn half_points<'de, D: Deserializer<'de>>(deserializer: D) -> Result<HalfPoints, D::Error> {
    let points = f64::deserialize(deserializer)?;
    HalfPoints::from_points(points)
        .ok_or_else(|| D::Error::custom(format!("{points} is not a count of half points")))
}

fn unless_blank<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    let text = String::deserialize(deserializer)?;
    if text.is_empty() {
        return Ok(None);
    }
    T::deserialize(text.into_deserializer()).map(Some)
}
