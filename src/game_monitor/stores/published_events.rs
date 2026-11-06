use sqlx::PgPool;

use crate::game_monitor::{GameEvent, GameEventKind};

use super::StoreError;

pub trait PublishedEventsStore: Clone + Send + Sync + 'static {
    fn record(
        &self,
        event: &GameEvent,
    ) -> impl Future<Output = Result<Recorded, StoreError>> + Send;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Recorded {
    First,
    Again,
}

#[derive(Clone, Debug)]
pub struct PostgresPublishedEventsStore {
    pool: PgPool,
}

impl PostgresPublishedEventsStore {
    #[must_use]
    pub const fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "game_event_kind", rename_all = "snake_case")]
enum StoredKind {
    GameStarted,
    GameFinished,
}

impl From<GameEventKind> for StoredKind {
    fn from(kind: GameEventKind) -> Self {
        match kind {
            GameEventKind::GameStarted => Self::GameStarted,
            GameEventKind::GameFinished => Self::GameFinished,
        }
    }
}

impl PublishedEventsStore for PostgresPublishedEventsStore {
    async fn record(&self, event: &GameEvent) -> Result<Recorded, StoreError> {
        let game_id = event.game().id.as_str();
        let kind = StoredKind::from(event.kind());
        let mut transaction = self.pool.begin().await?;
        let recorded = sqlx::query!(
            "INSERT INTO game_events (game_id, kind) VALUES ($1, $2) ON CONFLICT DO NOTHING",
            game_id,
            kind as StoredKind,
        )
        .execute(&mut *transaction)
        .await?;
        if recorded.rows_affected() == 0 {
            return Ok(Recorded::Again);
        }
        let fixtures_ids: Vec<i64> = event
            .matched
            .iter()
            .map(|matched| matched.fixtures_id.into_inner())
            .collect();
        let (whites, blacks): (Vec<&str>, Vec<&str>) = event
            .matched
            .iter()
            .map(|matched| {
                (
                    matched.fixture.white.as_str(),
                    matched.fixture.black.as_str(),
                )
            })
            .unzip();
        sqlx::query!(
            r#"
            INSERT INTO game_event_fixtures (game_id, kind, fixtures_id, white, black)
            SELECT $1::TEXT, $2::game_event_kind, * FROM UNNEST($3::BIGINT[], $4::TEXT[], $5::TEXT[])
            "#,
            game_id,
            kind as StoredKind,
            &fixtures_ids,
            &whites as &[&str],
            &blacks as &[&str],
        )
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(Recorded::First)
    }
}

#[cfg(test)]
mod tests;
