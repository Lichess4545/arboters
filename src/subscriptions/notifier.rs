mod rendering;

use kameo::Actor;
use kameo::actor::{ActorRef, Recipient};
use kameo::message::{Context, Message};
use url::Url;

use crate::chat::Messenger;
use crate::game_monitor::{Follow, GameEvent, MonitorUnavailable};
use crate::heltour::ActiveLeagues;

use super::Subscriptions;
use rendering::render;

#[derive(Debug)]
pub struct Notifier<R, M> {
    active: ActiveLeagues,
    subscriptions: R,
    messenger: M,
    lichess: Url,
}

impl<R: Subscriptions, M: Messenger> Notifier<R, M> {
    #[must_use]
    pub fn new(subscriptions: R, messenger: M, lichess: Url) -> Self {
        Self {
            active: ActiveLeagues::default(),
            subscriptions,
            messenger,
            lichess,
        }
    }

    async fn notify(&self, event: &GameEvent) {
        let game = event.game();
        let leagues: Vec<_> = self.active.leagues_of(game).into_iter().collect();
        if leagues.is_empty() {
            return;
        }
        let targets = match self
            .subscriptions
            .targets_for(&leagues, event.kind(), game.user_ids())
            .await
        {
            Ok(targets) => targets,
            Err(error) => {
                tracing::error!(error = %error, game_id = %game.id, "could not look up notification targets");
                return;
            }
        };
        let notification = render(event, &self.lichess);
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
