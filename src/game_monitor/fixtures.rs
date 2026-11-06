use std::collections::BTreeSet;

use nutype::nutype;
use time::OffsetDateTime;

use crate::lichess;

use super::League;

#[nutype(derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Display))]
pub struct FixturesId(i64);

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Fixture {
    pub white: lichess::UserId,
    pub black: lichess::UserId,
    pub time_control: Option<lichess::Clock>,
    pub variant: lichess::Variant,
    pub rated: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Period {
    pub starts: Option<OffsetDateTime>,
    pub ends: Option<OffsetDateTime>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewFixtures {
    pub league: League,
    pub period: Period,
    pub games: BTreeSet<Fixture>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fixtures {
    pub id: FixturesId,
    pub league: League,
    pub period: Period,
    pub games: BTreeSet<Fixture>,
    pub stored_at: OffsetDateTime,
}

impl Fixtures {
    #[must_use]
    pub fn stored(id: FixturesId, stored_at: OffsetDateTime, fixtures: NewFixtures) -> Self {
        Self {
            id,
            league: fixtures.league,
            period: fixtures.period,
            games: fixtures.games,
            stored_at,
        }
    }
}
