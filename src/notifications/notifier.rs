use std::convert::Infallible;

use kameo::Actor;
use kameo::actor::ActorRef;
use kameo::message::{Context, Message};

use crate::game_monitor::GameEvent;
use crate::heltour::league;
use crate::subscriptions::{SubscriptionSource, Subscriptions};

use super::messenger::Messenger;
use super::rendering::render;

#[derive(Debug)]
pub struct Notifier<R, M> {
    league: league::Tag,
    subscriptions: R,
    messenger: M,
}

impl<R: Subscriptions, M: Messenger> Notifier<R, M> {
    #[must_use]
    pub const fn new(league: league::Tag, subscriptions: R, messenger: M) -> Self {
        Self {
            league,
            subscriptions,
            messenger,
        }
    }

    async fn notify(&self, event: &GameEvent) {
        let game = event.game();
        let players = game
            .user_ids()
            .map(|user_id| SubscriptionSource::Player(user_id.clone()));
        let targets = match self
            .subscriptions
            .targets_for(&self.league, event.kind(), &players)
            .await
        {
            Ok(targets) => targets,
            Err(error) => {
                tracing::error!(error = %error, game_id = %game.id, "could not look up notification targets");
                return;
            }
        };
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

#[cfg(test)]
mod tests;
