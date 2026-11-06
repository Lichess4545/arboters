use nutype::nutype;
use serde::Deserialize;
use time::OffsetDateTime;

use crate::lichess;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pairing {
    pub white: lichess::UserId,
    pub black: lichess::UserId,
}

#[nutype(derive(Debug, Clone, Copy, PartialEq, Eq, Display, Deserialize))]
pub struct RoundId(i64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub struct Round {
    pub id: RoundId,
    #[serde(
        rename = "startsAt",
        default,
        with = "time::serde::timestamp::milliseconds::option"
    )]
    pub starts: Option<OffsetDateTime>,
    #[serde(
        rename = "endsAt",
        default,
        with = "time::serde::timestamp::milliseconds::option"
    )]
    pub ends: Option<OffsetDateTime>,
}

impl Round {
    #[must_use]
    pub fn has_ended_by(&self, now: OffsetDateTime) -> bool {
        self.ends.is_some_and(|ends| ends <= now)
    }
}

#[cfg(test)]
mod tests;
