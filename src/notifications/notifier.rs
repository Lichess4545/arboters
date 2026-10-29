use std::collections::BTreeSet;
use std::convert::Infallible;

use kameo::Actor;
use kameo::actor::ActorRef;
use kameo::message::{Context, Message};

use crate::chat::Messenger;
use crate::game_monitor::GameEvent;
use crate::heltour::CurrentPairings;
use crate::subscriptions::{SubscriptionSource, Subscriptions};

use super::rendering::render;

#[derive(Debug)]
pub struct Notifier<R, M> {
    pairings: CurrentPairings,
    subscriptions: R,
    messenger: M,
}

impl<R: Subscriptions, M: Messenger> Notifier<R, M> {
    #[must_use]
    pub fn new(subscriptions: R, messenger: M) -> Self {
        Self {
            pairings: CurrentPairings::default(),
            subscriptions,
            messenger,
        }
    }

    async fn notify(&self, event: &GameEvent) {
        let game = event.game();
        let [white, black] = game
            .user_ids()
            .map(|user_id| SubscriptionSource::Player(user_id.clone()));
        let sources = [white, black, SubscriptionSource::League];
        let mut targets = BTreeSet::new();
        for league in self.pairings.leagues_of(game) {
            match self
                .subscriptions
                .targets_for(league, event.kind(), &sources)
                .await
            {
                Ok(found) => targets.extend(found),
                Err(error) => {
                    tracing::error!(error = %error, game_id = %game.id, %league, "could not look up notification targets");
                    return;
                }
            }
        }
        if targets.is_empty() {
            return;
        }
        let notification = render(event);
        for target in &targets {
            match self.messenger.send(target, &notification).await {
                Ok(()) => tracing::debug!(game_id = %game.id, ?target, "notification delivered"),
                Err(error) => tracing::warn!(
                    error = %error,
                    cause = %error.cause,
                    game_id = %game.id,
                    ?target,
                    "notification delivery failed"
                ),
            }
        }
    }
}

impl<R: Subscriptions, M: Messenger> Actor for Notifier<R, M> {
    type Args = Self;
    type Error = Infallible;

    fn on_start(
        notifier: Self,
        _: ActorRef<Self>,
    ) -> impl Future<Output = Result<Self, Self::Error>> + Send {
        std::future::ready(Ok(notifier))
    }
}

impl<R: Subscriptions, M: Messenger> Message<GameEvent> for Notifier<R, M> {
    type Reply = ();

    async fn handle(&mut self, event: GameEvent, _: &mut Context<Self, Self::Reply>) {
        self.notify(&event).await;
    }
}

impl<R: Subscriptions, M: Messenger> Message<CurrentPairings> for Notifier<R, M> {
    type Reply = ();

    async fn handle(&mut self, pairings: CurrentPairings, _: &mut Context<Self, Self::Reply>) {
        self.pairings = pairings;
    }
}

#[cfg(test)]
mod tests;
