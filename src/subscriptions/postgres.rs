use std::collections::BTreeSet;

use sqlx::PgPool;

use crate::chat::{ChannelName, ChatUserId, NotificationTarget, TopicName};
use crate::game_monitor::GameEventKind;
use crate::heltour::{league, team};
use crate::lichess;

use super::model::{NewSubscription, Remover, Subscription, SubscriptionId, SubscriptionSource};
use super::{Subscriptions, SubscriptionsError};

#[derive(Clone, Debug)]
pub struct PostgresSubscriptions {
    pool: PgPool,
}

impl PostgresSubscriptions {
    #[must_use]
    pub const fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "subscription_event", rename_all = "snake_case")]
enum StoredEvent {
    GameStarted,
    GameFinished,
}

impl From<GameEventKind> for StoredEvent {
    fn from(kind: GameEventKind) -> Self {
        match kind {
            GameEventKind::GameStarted => Self::GameStarted,
            GameEventKind::GameFinished => Self::GameFinished,
        }
    }
}

impl From<StoredEvent> for GameEventKind {
    fn from(event: StoredEvent) -> Self {
        match event {
            StoredEvent::GameStarted => Self::GameStarted,
            StoredEvent::GameFinished => Self::GameFinished,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "subscription_source_kind", rename_all = "snake_case")]
enum StoredSourceKind {
    Player,
    Team,
    League,
}

fn stored_source<'a>(
    source: &'a SubscriptionSource,
    league: &'a league::Tag,
) -> (StoredSourceKind, &'a str) {
    match source {
        SubscriptionSource::Player(user_id) => (StoredSourceKind::Player, user_id.as_str()),
        SubscriptionSource::Team(team) => (StoredSourceKind::Team, team.as_str()),
        SubscriptionSource::League => (StoredSourceKind::League, league.as_str()),
    }
}

fn source_from(kind: StoredSourceKind, name: &str) -> Result<SubscriptionSource, sqlx::Error> {
    Ok(match kind {
        StoredSourceKind::Player => {
            SubscriptionSource::Player(lichess::UserId::try_new(name).map_err(undecodable)?)
        }
        StoredSourceKind::Team => {
            SubscriptionSource::Team(team::Name::try_new(name).map_err(undecodable)?)
        }
        StoredSourceKind::League => SubscriptionSource::League,
    })
}

fn stored_user_id(user_id: ChatUserId) -> Result<i64, sqlx::Error> {
    i64::try_from(user_id.0).map_err(|error| sqlx::Error::Encode(Box::new(error)))
}

fn undecodable(error: impl std::error::Error + Send + Sync + 'static) -> sqlx::Error {
    sqlx::Error::Decode(Box::new(error))
}

struct StoredTarget {
    user_id: Option<i64>,
    channel: Option<String>,
    topic: Option<String>,
}

impl StoredTarget {
    fn from_target(target: &NotificationTarget) -> Result<Self, sqlx::Error> {
        Ok(match target {
            NotificationTarget::DirectMessage { user_id } => Self {
                user_id: Some(stored_user_id(*user_id)?),
                channel: None,
                topic: None,
            },
            NotificationTarget::Channel { channel, topic } => Self {
                user_id: None,
                channel: Some(channel.as_str().to_owned()),
                topic: Some(topic.as_str().to_owned()),
            },
        })
    }

    fn into_target(self) -> Result<NotificationTarget, sqlx::Error> {
        match (self.user_id, self.channel, self.topic) {
            (Some(user_id), None, None) => Ok(NotificationTarget::DirectMessage {
                user_id: ChatUserId(u64::try_from(user_id).map_err(undecodable)?),
            }),
            (None, Some(channel), Some(topic)) => Ok(NotificationTarget::Channel {
                channel: ChannelName::try_new(&channel).map_err(undecodable)?,
                topic: TopicName::try_new(&topic).map_err(undecodable)?,
            }),
            _ => Err(sqlx::Error::Decode(
                "stored target is neither a direct message nor a channel topic".into(),
            )),
        }
    }
}

struct StoredSubscription {
    id: i64,
    requester_user_id: i64,
    league: String,
    source_kind: StoredSourceKind,
    source_name: String,
    event: StoredEvent,
    target_user_id: Option<i64>,
    target_channel: Option<String>,
    target_topic: Option<String>,
}

impl StoredSubscription {
    fn into_subscription(self) -> Result<Subscription, sqlx::Error> {
        let target = StoredTarget {
            user_id: self.target_user_id,
            channel: self.target_channel,
            topic: self.target_topic,
        };
        Ok(Subscription {
            id: SubscriptionId(self.id),
            requester: ChatUserId(u64::try_from(self.requester_user_id).map_err(undecodable)?),
            league: league::Tag::try_new(&self.league).map_err(undecodable)?,
            source: source_from(self.source_kind, &self.source_name)?,
            event: self.event.into(),
            target: target.into_target()?,
        })
    }
}

impl Subscriptions for PostgresSubscriptions {
    async fn subscribe(
        &self,
        subscription: NewSubscription,
    ) -> Result<SubscriptionId, SubscriptionsError> {
        let (source_kind, source_name) = stored_source(&subscription.source, &subscription.league);
        let target = StoredTarget::from_target(&subscription.target)?;
        let id = sqlx::query_scalar!(
            r#"
            INSERT INTO subscriptions (
                requester_user_id, league, source_kind, source_name, event,
                target_user_id, target_channel, target_topic
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            ON CONFLICT (
                requester_user_id, league, source_kind, source_name, event,
                target_user_id, target_channel, target_topic
            )
            DO UPDATE SET requester_user_id = EXCLUDED.requester_user_id
            RETURNING id
            "#,
            stored_user_id(subscription.requester)?,
            subscription.league.as_str(),
            source_kind as StoredSourceKind,
            source_name,
            StoredEvent::from(subscription.event) as StoredEvent,
            target.user_id,
            target.channel,
            target.topic,
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(SubscriptionId(id))
    }

    async fn targets_for(
        &self,
        league: &league::Tag,
        event: GameEventKind,
        sources: &[SubscriptionSource],
    ) -> Result<BTreeSet<NotificationTarget>, SubscriptionsError> {
        let mut targets = BTreeSet::new();
        for source in sources {
            let (source_kind, source_name) = stored_source(source, league);
            let rows = sqlx::query!(
                r#"
                SELECT target_user_id, target_channel, target_topic
                FROM subscriptions
                WHERE league = $1 AND event = $2 AND source_kind = $3 AND source_name = $4
                "#,
                league.as_str(),
                StoredEvent::from(event) as StoredEvent,
                source_kind as StoredSourceKind,
                source_name,
            )
            .fetch_all(&self.pool)
            .await?;
            for row in rows {
                let stored = StoredTarget {
                    user_id: row.target_user_id,
                    channel: row.target_channel,
                    topic: row.target_topic,
                };
                targets.insert(stored.into_target()?);
            }
        }
        Ok(targets)
    }

    async fn requested_by(
        &self,
        requester: ChatUserId,
    ) -> Result<Vec<Subscription>, SubscriptionsError> {
        let rows = sqlx::query_as!(
            StoredSubscription,
            r#"
            SELECT
                id, requester_user_id, league,
                source_kind AS "source_kind: StoredSourceKind", source_name,
                event AS "event: StoredEvent", target_user_id, target_channel, target_topic
            FROM subscriptions
            WHERE requester_user_id = $1
            ORDER BY id
            "#,
            stored_user_id(requester)?,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(StoredSubscription::into_subscription)
            .collect::<Result<_, _>>()?)
    }

    async fn posting_to(
        &self,
        target: &NotificationTarget,
    ) -> Result<Vec<Subscription>, SubscriptionsError> {
        let target = StoredTarget::from_target(target)?;
        let rows = sqlx::query_as!(
            StoredSubscription,
            r#"
            SELECT
                id, requester_user_id, league,
                source_kind AS "source_kind: StoredSourceKind", source_name,
                event AS "event: StoredEvent", target_user_id, target_channel, target_topic
            FROM subscriptions
            WHERE target_user_id IS NOT DISTINCT FROM $1
                AND target_channel IS NOT DISTINCT FROM $2
                AND target_topic IS NOT DISTINCT FROM $3
            ORDER BY id
            "#,
            target.user_id,
            target.channel,
            target.topic,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(StoredSubscription::into_subscription)
            .collect::<Result<_, _>>()?)
    }

    async fn remove(
        &self,
        id: SubscriptionId,
        remover: Remover,
    ) -> Result<bool, SubscriptionsError> {
        let (owner, at) = match &remover {
            Remover::Owner { user, at } => {
                (Some(stored_user_id(*user)?), StoredTarget::from_target(at)?)
            }
            Remover::Moderator => (
                None,
                StoredTarget {
                    user_id: None,
                    channel: None,
                    topic: None,
                },
            ),
        };
        let deleted = sqlx::query!(
            r#"
            DELETE FROM subscriptions
            WHERE id = $1
                AND (
                    $2::BIGINT IS NULL
                    OR requester_user_id = $2
                    OR (
                        target_user_id IS NOT DISTINCT FROM $3::BIGINT
                        AND target_channel IS NOT DISTINCT FROM $4::TEXT
                        AND target_topic IS NOT DISTINCT FROM $5::TEXT
                    )
                )
            "#,
            id.0,
            owner,
            at.user_id,
            at.channel,
            at.topic,
        )
        .execute(&self.pool)
        .await?;
        Ok(deleted.rows_affected() > 0)
    }
}

#[cfg(test)]
mod tests;
