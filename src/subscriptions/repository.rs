use std::collections::BTreeSet;

use crate::games::{GameEventKind, LichessUsername};

use super::model::{NewSubscription, NotificationTarget, SubscriptionId};

#[derive(Debug, thiserror::Error)]
pub enum RepositoryError {
    #[error("database query failed")]
    Database(#[from] sqlx::Error),
    #[error("stored subscription {id} is corrupt: {reason}")]
    CorruptRow { id: SubscriptionId, reason: String },
}

pub trait SubscriptionRepository: Clone + Send + Sync + 'static {
    fn subscribe(
        &self,
        subscription: NewSubscription,
    ) -> impl Future<Output = Result<SubscriptionId, RepositoryError>> + Send;

    fn targets_for(
        &self,
        event: GameEventKind,
        sources: &[LichessUsername],
    ) -> impl Future<Output = Result<BTreeSet<NotificationTarget>, RepositoryError>> + Send;

    fn watched_usernames(
        &self,
    ) -> impl Future<Output = Result<BTreeSet<LichessUsername>, RepositoryError>> + Send;
}
