use std::collections::{BTreeMap, BTreeSet};

use sqlx::PgPool;
use time::OffsetDateTime;

use crate::game_monitor::league::{Key, Source};
use crate::game_monitor::{Fixture, Fixtures, FixturesId, League, NewFixtures, Period};
use crate::lichess;

use super::StoreError;

pub trait FixturesStore: Clone + Send + Sync + 'static {
    fn active(&self) -> impl Future<Output = Result<Vec<Fixtures>, StoreError>> + Send;

    fn store(
        &self,
        fixtures: NewFixtures,
    ) -> impl Future<Output = Result<Fixtures, StoreError>> + Send;

    fn complete(&self, id: FixturesId) -> impl Future<Output = Result<(), StoreError>> + Send;
}

#[derive(Clone, Debug)]
pub struct PostgresFixturesStore {
    pool: PgPool,
}

impl PostgresFixturesStore {
    #[must_use]
    pub const fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "variant", rename_all = "snake_case")]
enum StoredVariant {
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

impl From<lichess::Variant> for StoredVariant {
    fn from(variant: lichess::Variant) -> Self {
        match variant {
            lichess::Variant::Standard => Self::Standard,
            lichess::Variant::Chess960 => Self::Chess960,
            lichess::Variant::Crazyhouse => Self::Crazyhouse,
            lichess::Variant::Antichess => Self::Antichess,
            lichess::Variant::Atomic => Self::Atomic,
            lichess::Variant::Horde => Self::Horde,
            lichess::Variant::KingOfTheHill => Self::KingOfTheHill,
            lichess::Variant::RacingKings => Self::RacingKings,
            lichess::Variant::ThreeCheck => Self::ThreeCheck,
            lichess::Variant::FromPosition => Self::FromPosition,
        }
    }
}

impl From<StoredVariant> for lichess::Variant {
    fn from(variant: StoredVariant) -> Self {
        match variant {
            StoredVariant::Standard => Self::Standard,
            StoredVariant::Chess960 => Self::Chess960,
            StoredVariant::Crazyhouse => Self::Crazyhouse,
            StoredVariant::Antichess => Self::Antichess,
            StoredVariant::Atomic => Self::Atomic,
            StoredVariant::Horde => Self::Horde,
            StoredVariant::KingOfTheHill => Self::KingOfTheHill,
            StoredVariant::RacingKings => Self::RacingKings,
            StoredVariant::ThreeCheck => Self::ThreeCheck,
            StoredVariant::FromPosition => Self::FromPosition,
        }
    }
}

struct StoredFixtures {
    id: i64,
    source: String,
    key: String,
    starts_at: Option<OffsetDateTime>,
    ends_at: Option<OffsetDateTime>,
    stored_at: OffsetDateTime,
}

impl StoredFixtures {
    fn into_fixtures(self) -> Fixtures {
        Fixtures {
            id: FixturesId::new(self.id),
            league: League {
                source: Source::new(self.source),
                key: Key::new(self.key),
            },
            period: Period {
                starts: self.starts_at,
                ends: self.ends_at,
            },
            games: BTreeSet::new(),
            stored_at: self.stored_at,
        }
    }
}

struct StoredFixture {
    fixtures_id: i64,
    white: String,
    black: String,
    initial_seconds: Option<i64>,
    increment_seconds: Option<i64>,
    variant: StoredVariant,
    rated: bool,
}

impl StoredFixture {
    fn into_fixture(self) -> Result<Fixture, sqlx::Error> {
        Ok(Fixture {
            white: lichess::UserId::try_new(&self.white).map_err(undecodable)?,
            black: lichess::UserId::try_new(&self.black).map_err(undecodable)?,
            time_control: self.time_control()?,
            variant: self.variant.into(),
            rated: self.rated,
        })
    }

    fn time_control(&self) -> Result<Option<lichess::Clock>, sqlx::Error> {
        let (Some(initial), Some(increment)) = (self.initial_seconds, self.increment_seconds)
        else {
            return Ok(None);
        };
        Ok(Some(lichess::Clock {
            initial_seconds: u32::try_from(initial).map_err(undecodable)?,
            increment_seconds: u32::try_from(increment).map_err(undecodable)?,
        }))
    }
}

fn undecodable(error: impl std::error::Error + Send + Sync + 'static) -> sqlx::Error {
    sqlx::Error::Decode(Box::new(error))
}

struct StoredGames<'a> {
    whites: Vec<&'a str>,
    blacks: Vec<&'a str>,
    initial_seconds: Vec<Option<i64>>,
    increment_seconds: Vec<Option<i64>>,
    variants: Vec<StoredVariant>,
    rated: Vec<bool>,
}

impl<'a> StoredGames<'a> {
    fn of(games: &'a BTreeSet<Fixture>) -> Self {
        Self {
            whites: games.iter().map(|game| game.white.as_str()).collect(),
            blacks: games.iter().map(|game| game.black.as_str()).collect(),
            initial_seconds: games
                .iter()
                .map(|game| game.time_control.map(|clock| clock.initial_seconds.into()))
                .collect(),
            increment_seconds: games
                .iter()
                .map(|game| {
                    game.time_control
                        .map(|clock| clock.increment_seconds.into())
                })
                .collect(),
            variants: games.iter().map(|game| game.variant.into()).collect(),
            rated: games.iter().map(|game| game.rated).collect(),
        }
    }
}

impl FixturesStore for PostgresFixturesStore {
    async fn active(&self) -> Result<Vec<Fixtures>, StoreError> {
        let rows = sqlx::query_as!(
            StoredFixtures,
            r#"
            SELECT fixtures.id, source, key, starts_at, ends_at, stored_at
            FROM fixtures
            JOIN leagues ON leagues.id = fixtures.league_id
            WHERE replaced_at IS NULL AND completed_at IS NULL
            "#,
        )
        .fetch_all(&self.pool)
        .await?;
        let ids: Vec<i64> = rows.iter().map(|row| row.id).collect();
        let games = sqlx::query_as!(
            StoredFixture,
            r#"
            SELECT fixtures_id, white, black, initial_seconds, increment_seconds,
                variant AS "variant: StoredVariant", rated
            FROM fixture_games
            WHERE fixtures_id = ANY($1)
            "#,
            &ids,
        )
        .fetch_all(&self.pool)
        .await?;
        let mut fixtures: BTreeMap<i64, Fixtures> = rows
            .into_iter()
            .map(|row| (row.id, row.into_fixtures()))
            .collect();
        for game in games {
            if let Some(fixtures) = fixtures.get_mut(&game.fixtures_id) {
                fixtures.games.insert(game.into_fixture()?);
            }
        }
        Ok(fixtures.into_values().collect())
    }

    async fn store(&self, fixtures: NewFixtures) -> Result<Fixtures, StoreError> {
        let mut transaction = self.pool.begin().await?;
        let league_id = sqlx::query_scalar!(
            r#"
            INSERT INTO leagues (source, key) VALUES ($1, $2)
            ON CONFLICT (source, key) DO UPDATE SET source = EXCLUDED.source
            RETURNING id
            "#,
            fixtures.league.source.as_str(),
            fixtures.league.key.as_str(),
        )
        .fetch_one(&mut *transaction)
        .await?;
        sqlx::query!(
            "UPDATE fixtures SET replaced_at = now() WHERE league_id = $1 AND replaced_at IS NULL",
            league_id,
        )
        .execute(&mut *transaction)
        .await?;
        let stored = sqlx::query!(
            "INSERT INTO fixtures (league_id, starts_at, ends_at) VALUES ($1, $2, $3) RETURNING id, stored_at",
            league_id,
            fixtures.period.starts,
            fixtures.period.ends,
        )
        .fetch_one(&mut *transaction)
        .await?;
        let games = StoredGames::of(&fixtures.games);
        sqlx::query!(
            r#"
            INSERT INTO fixture_games
                (fixtures_id, white, black, initial_seconds, increment_seconds, variant, rated)
            SELECT $1, * FROM UNNEST(
                $2::TEXT[], $3::TEXT[], $4::BIGINT[], $5::BIGINT[], $6::variant[], $7::BOOLEAN[]
            )
            "#,
            stored.id,
            &games.whites as &[&str],
            &games.blacks as &[&str],
            &games.initial_seconds as &[Option<i64>],
            &games.increment_seconds as &[Option<i64>],
            &games.variants as &[StoredVariant],
            &games.rated,
        )
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(Fixtures::stored(
            FixturesId::new(stored.id),
            stored.stored_at,
            fixtures,
        ))
    }

    async fn complete(&self, id: FixturesId) -> Result<(), StoreError> {
        sqlx::query!(
            "UPDATE fixtures SET completed_at = now() WHERE id = $1 AND completed_at IS NULL",
            id.into_inner(),
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
