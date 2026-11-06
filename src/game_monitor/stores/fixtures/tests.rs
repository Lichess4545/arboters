use time::macros::datetime;

use super::*;
use crate::testing::{fixture, league};

const EVERY_VARIANT: [lichess::Variant; 10] = [
    lichess::Variant::Standard,
    lichess::Variant::Chess960,
    lichess::Variant::Crazyhouse,
    lichess::Variant::Antichess,
    lichess::Variant::Atomic,
    lichess::Variant::Horde,
    lichess::Variant::KingOfTheHill,
    lichess::Variant::RacingKings,
    lichess::Variant::ThreeCheck,
    lichess::Variant::FromPosition,
];

fn round_three(league: League, games: impl IntoIterator<Item = Fixture>) -> NewFixtures {
    NewFixtures {
        league,
        period: Period {
            starts: Some(datetime!(2026-11-02 18:00:00.123 UTC)),
            ends: Some(datetime!(2026-11-09 18:00:00 UTC)),
        },
        games: games.into_iter().collect(),
    }
}

#[sqlx::test]
async fn stored_fixtures_read_back_as_active(pool: PgPool) {
    let fixtures_store = PostgresFixturesStore::new(pool);
    let new = round_three(
        league("heltour", "team4545"),
        EVERY_VARIANT
            .map(|variant| Fixture {
                variant,
                ..fixture(&format!("{variant:?}"), "bob")
            })
            .into_iter()
            .chain([Fixture {
                time_control: None,
                rated: false,
                ..fixture("carol", "dave")
            }]),
    );

    let stored = fixtures_store.store(new).await.unwrap();

    assert_eq!(fixtures_store.active().await.unwrap(), [stored]);
}

#[sqlx::test]
async fn new_fixtures_replace_only_the_fixtures_of_their_source_and_key(pool: PgPool) {
    let fixtures_store = PostgresFixturesStore::new(pool);
    fixtures_store
        .store(round_three(
            league("heltour", "team4545"),
            [fixture("alice", "bob")],
        ))
        .await
        .unwrap();
    let other_key = fixtures_store
        .store(round_three(
            league("heltour", "lonewolf"),
            [fixture("alice", "bob")],
        ))
        .await
        .unwrap();
    let other_source = fixtures_store
        .store(round_three(
            league("sheets", "team4545"),
            [fixture("alice", "bob")],
        ))
        .await
        .unwrap();

    let replacement = fixtures_store
        .store(NewFixtures {
            period: Period::default(),
            ..round_three(league("heltour", "team4545"), [])
        })
        .await
        .unwrap();

    assert_eq!(
        fixtures_store.active().await.unwrap(),
        [other_key, other_source, replacement]
    );
}

#[sqlx::test]
async fn completed_fixtures_are_inactive_and_can_be_replaced(pool: PgPool) {
    let fixtures_store = PostgresFixturesStore::new(pool);
    let completed = fixtures_store
        .store(round_three(
            league("heltour", "team4545"),
            [fixture("alice", "bob")],
        ))
        .await
        .unwrap();

    fixtures_store.complete(completed.id).await.unwrap();

    assert_eq!(fixtures_store.active().await.unwrap(), []);

    let replacement = fixtures_store
        .store(round_three(
            league("heltour", "team4545"),
            [fixture("carol", "dave")],
        ))
        .await
        .unwrap();

    assert_eq!(fixtures_store.active().await.unwrap(), [replacement]);
}
