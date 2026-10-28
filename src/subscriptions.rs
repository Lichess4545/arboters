use std::collections::BTreeSet;

use crate::game_monitor::GameEventKind;
use crate::heltour::league;

mod model;
mod postgres;

pub use model::{
    ChannelName, ChatUserId, NewSubscription, NotificationTarget, SubscriptionId,
    SubscriptionSource, TopicName, TopicNameError,
};
pub use postgres::PostgresSubscriptions;

#[derive(Debug, thiserror::Error)]
#[error("subscriptions query failed")]
pub struct SubscriptionsError(#[from] sqlx::Error);

pub trait Subscriptions: Clone + Send + Sync + 'static {
    fn subscribe(
        &self,
        subscription: NewSubscription,
    ) -> impl Future<Output = Result<SubscriptionId, SubscriptionsError>> + Send;

    fn targets_for(
        &self,
        league: &league::Tag,
        event: GameEventKind,
        sources: &[SubscriptionSource],
    ) -> impl Future<Output = Result<BTreeSet<NotificationTarget>, SubscriptionsError>> + Send;
}
