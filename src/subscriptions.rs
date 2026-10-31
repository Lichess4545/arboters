use std::collections::BTreeSet;

use crate::chat::{ChatUserId, Conversation};
use crate::game_monitor::GameEventKind;
use crate::heltour::league;

mod commands;
mod model;
mod notifier;
mod postgres;

pub use commands::{InboxUnavailable, SubscriptionCommands};
pub use model::{NewSubscription, Remover, Subscription, SubscriptionId, SubscriptionSource};
pub use notifier::Notifier;
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
    ) -> impl Future<Output = Result<BTreeSet<Conversation>, SubscriptionsError>> + Send;

    fn requested_by(
        &self,
        requester: ChatUserId,
    ) -> impl Future<Output = Result<Vec<Subscription>, SubscriptionsError>> + Send;

    fn posting_to(
        &self,
        target: &Conversation,
    ) -> impl Future<Output = Result<Vec<Subscription>, SubscriptionsError>> + Send;

    fn remove(
        &self,
        id: SubscriptionId,
        remover: Remover,
    ) -> impl Future<Output = Result<bool, SubscriptionsError>> + Send;
}
