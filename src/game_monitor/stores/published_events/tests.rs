use std::collections::BTreeSet;

use super::*;
use crate::game_monitor::{
    Decision, Draw, FixturesStore, GameChange, MatchedFixture, NewFixtures, Period,
    PostgresFixturesStore,
};
use crate::testing::{alice_vs_bob, fixture, league};

#[sqlx::test]
async fn each_game_and_kind_is_recorded_first_then_again(pool: PgPool) {
    let fixtures = PostgresFixturesStore::new(pool.clone())
        .store(NewFixtures {
            league: league("heltour", "team4545"),
            period: Period::default(),
            games: BTreeSet::from([fixture("alice", "bob")]),
        })
        .await
        .unwrap();
    let published_events = PostgresPublishedEventsStore::new(pool);
    let started = GameEvent {
        change: GameChange::Started(alice_vs_bob("g1")),
        matched: vec![MatchedFixture {
            fixtures_id: fixtures.id,
            league: fixtures.league,
            fixture: fixture("alice", "bob"),
        }],
    };
    let finished = GameEvent {
        change: GameChange::Decided {
            game: alice_vs_bob("g1"),
            decision: Decision::Drawn(Draw::AgreementOrRule),
        },
        matched: Vec::new(),
    };
    let other_game = GameEvent {
        change: GameChange::Started(alice_vs_bob("g2")),
        matched: Vec::new(),
    };

    let recorded = [
        published_events.record(&started).await.unwrap(),
        published_events.record(&started).await.unwrap(),
        published_events.record(&finished).await.unwrap(),
        published_events.record(&other_game).await.unwrap(),
        published_events.record(&finished).await.unwrap(),
    ];

    assert_eq!(
        recorded,
        [
            Recorded::First,
            Recorded::Again,
            Recorded::First,
            Recorded::First,
            Recorded::Again
        ]
    );
}
