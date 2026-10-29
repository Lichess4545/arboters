use std::collections::BTreeSet;

use kameo::Actor;
use kameo::actor::{ActorRef, Recipient};
use kameo::message::{Context, Message};

use crate::chat::Messenger;
use crate::game_monitor::{Follow, GameEvent, MonitorUnavailable};
use crate::heltour::ActiveLeagues;
use crate::subscriptions::{SubscriptionSource, Subscriptions};

use super::rendering::render;

#[derive(Debug)]
pub struct Notifier<R, M> {
    active: ActiveLeagues,
    subscriptions: R,
    messenger: M,
}

impl<R: Subscriptions, M: Messenger> Notifier<R, M> {
    #[must_use]
    pub fn new(subscriptions: R, messenger: M) -> Self {
        Self {
            active: ActiveLeagues::default(),
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
        for league in self.active.leagues_of(game) {
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
    type Args = (Self, Recipient<Follow<GameEvent>>);
    type Error = MonitorUnavailable;

    async fn on_start(
        (notifier, monitor): Self::Args,
        actor_ref: ActorRef<Self>,
    ) -> Result<Self, Self::Error> {
        monitor
            .tell(Follow(actor_ref.recipient::<GameEvent>()))
            .await?;
        Ok(notifier)
    }
}

impl<R: Subscriptions, M: Messenger> Message<GameEvent> for Notifier<R, M> {
    type Reply = ();

    async fn handle(&mut self, event: GameEvent, _: &mut Context<Self, Self::Reply>) {
        self.notify(&event).await;
    }
}

impl<R: Subscriptions, M: Messenger> Message<ActiveLeagues> for Notifier<R, M> {
    type Reply = ();

    async fn handle(&mut self, active: ActiveLeagues, _: &mut Context<Self, Self::Reply>) {
        self.active = active;
    }
}

#[cfg(test)]
mod tests;
