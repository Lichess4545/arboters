mod rendering;

use std::convert::Infallible;

use kameo::Actor;
use kameo::actor::ActorRef;
use kameo::message::{Context, Message, StreamMessage};
use tokio::sync::{broadcast, watch};
use url::Url;

use crate::chat::Messenger;
use crate::game_monitor::{self, GameEvent};
use crate::heltour::{ActiveLeagues, league};

use super::Subscriptions;
use rendering::render;

#[derive(Debug)]
pub struct Notifier<R, M> {
    active_leagues: watch::Receiver<ActiveLeagues>,
    subscriptions: R,
    messenger: M,
    lichess: Url,
}

impl<R: Subscriptions, M: Messenger> Notifier<R, M> {
    #[must_use]
    pub const fn new(
        subscriptions: R,
        messenger: M,
        lichess: Url,
        active_leagues: watch::Receiver<ActiveLeagues>,
    ) -> Self {
        Self {
            active_leagues,
            subscriptions,
            messenger,
            lichess,
        }
    }

    async fn notify(&self, event: &GameEvent) {
        let game = event.game();
        let leagues: Vec<league::Tag> = self
            .active_leagues
            .borrow()
            .leagues_of(game)
            .into_iter()
            .cloned()
            .collect();
        if leagues.is_empty() {
            return;
        }
        let leagues: Vec<_> = leagues.iter().collect();
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
    type Args = (Self, broadcast::Receiver<GameEvent>);
    type Error = Infallible;

    fn on_start(
        (notifier, events): Self::Args,
        actor_ref: ActorRef<Self>,
    ) -> impl Future<Output = Result<Self, Self::Error>> + Send {
        actor_ref.attach_stream(game_monitor::event_stream(events), (), ());
        std::future::ready(Ok(notifier))
    }
}

impl<R: Subscriptions, M: Messenger> Message<StreamMessage<GameEvent, (), ()>> for Notifier<R, M> {
    type Reply = ();

    async fn handle(
        &mut self,
        message: StreamMessage<GameEvent, (), ()>,
        _: &mut Context<Self, Self::Reply>,
    ) {
        if let StreamMessage::Next(event) = message {
            self.notify(&event).await;
        }
    }
}

#[cfg(test)]
mod tests;
