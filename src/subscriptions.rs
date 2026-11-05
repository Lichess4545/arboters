use std::collections::BTreeSet;

use crate::chat::Conversation;
use crate::game_monitor::GameEventKind;
use crate::heltour::league;
use crate::lichess;

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
        leagues: &[&league::Tag],
        event: GameEventKind,
        players: [&lichess::UserId; 2],
    ) -> impl Future<Output = Result<BTreeSet<Conversation>, SubscriptionsError>> + Send;

    fn listed_in(
        &self,
        conversation: &Conversation,
    ) -> impl Future<Output = Result<Vec<Subscription>, SubscriptionsError>> + Send;

    fn remove(
        &self,
        id: SubscriptionId,
        remover: Remover,
    ) -> impl Future<Output = Result<bool, SubscriptionsError>> + Send;
}
