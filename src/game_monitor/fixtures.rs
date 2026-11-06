use std::collections::BTreeSet;

use nutype::nutype;
use time::OffsetDateTime;

use crate::lichess;

use super::League;
use super::event::MatchedFixture;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Difference {
    ColorsFlipped,
    WrongTimeControl,
    WrongVariant,
    WrongRatedSetting,
}

impl Fixture {
    fn is_played_in(&self, game: &lichess::Game) -> bool {
        let players = game.user_ids();
        players == [&self.white, &self.black] || players == [&self.black, &self.white]
    }

    /// ```
    /// use std::collections::BTreeSet;
    ///
    /// use arboters::game_monitor::{Difference, Fixture};
    /// use arboters::lichess::{Clock, Game, GameId, Player, Speed, UserId, Variant};
    ///
    /// let player = |name| Player { user_id: UserId::try_new(name).unwrap(), rating: None };
    /// let classical = Some(Clock { initial_seconds: 2700, increment_seconds: 45 });
    /// let fixture = Fixture {
    ///     white: UserId::try_new("alice").unwrap(),
    ///     black: UserId::try_new("bob").unwrap(),
    ///     time_control: classical,
    ///     variant: Variant::Standard,
    ///     rated: true,
    /// };
    /// let game = Game {
    ///     id: GameId::new("abcd1234"),
    ///     white: player("alice"),
    ///     black: player("bob"),
    ///     speed: Speed::Classical,
    ///     rated: true,
    ///     variant: Variant::Standard,
    ///     clock: classical,
    /// };
    /// assert_eq!(fixture.differences_from(&game), BTreeSet::new());
    ///
    /// let rematch = Game { white: player("bob"), black: player("alice"), rated: false, ..game };
    /// assert_eq!(
    ///     fixture.differences_from(&rematch),
    ///     BTreeSet::from([Difference::ColorsFlipped, Difference::WrongRatedSetting]),
    /// );
    /// ```
    #[must_use]
    pub fn differences_from(&self, game: &lichess::Game) -> BTreeSet<Difference> {
        let mut differences = BTreeSet::new();
        if game.white.user_id == self.black {
            differences.insert(Difference::ColorsFlipped);
        }
        if self
            .time_control
            .is_none_or(|time_control| game.clock != Some(time_control))
        {
            differences.insert(Difference::WrongTimeControl);
        }
        if game.variant != self.variant {
            differences.insert(Difference::WrongVariant);
        }
        if game.rated != self.rated {
            differences.insert(Difference::WrongRatedSetting);
        }
        differences
    }
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

    #[must_use]
    pub fn played_since(&self) -> OffsetDateTime {
        self.period.starts.unwrap_or(self.stored_at)
    }

    pub(super) fn has_the_games_and_period_of(&self, new: &NewFixtures) -> bool {
        self.league == new.league && self.period == new.period && self.games == new.games
    }

    pub(super) fn players(&self) -> impl Iterator<Item = &lichess::UserId> {
        self.games
            .iter()
            .flat_map(|game| [&game.white, &game.black])
    }

    pub(super) fn played_in<'a>(
        &'a self,
        game: &'a lichess::Game,
    ) -> impl Iterator<Item = MatchedFixture> + 'a {
        self.games
            .iter()
            .filter(|fixture| fixture.is_played_in(game))
            .map(|fixture| MatchedFixture {
                fixtures_id: self.id,
                league: self.league.clone(),
                fixture: fixture.clone(),
            })
    }
}
