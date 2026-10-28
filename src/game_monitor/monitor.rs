use std::collections::BTreeSet;
use std::convert::Infallible;

use kameo::Actor;
use kameo::actor::{ActorRef, Recipient};
use kameo::message::{Context, Message};

use crate::lichess;

use super::connection::Connection;
use super::event::GameEvent;
use super::settings::StreamSettings;
use super::stream::{GameObservation, LichessStream};
use super::{StreamConnected, Watch};

#[derive(Debug)]
pub struct GameMonitor {
    stream: LichessStream,
    settings: StreamSettings,
    listener: Recipient<GameEvent>,
    connected: Recipient<StreamConnected>,
    watched: BTreeSet<lichess::UserId>,
    connections: Vec<Connection>,
}

impl GameMonitor {
    #[must_use]
    pub const fn new(
        stream: LichessStream,
        settings: StreamSettings,
        listener: Recipient<GameEvent>,
        connected: Recipient<StreamConnected>,
    ) -> Self {
        Self {
            stream,
            settings,
            listener,
            connected,
            watched: BTreeSet::new(),
            connections: Vec::new(),
        }
    }

    fn reconnect(&mut self, monitor: &ActorRef<Self>) {
        self.connections.clear();
        let observations = monitor.clone().recipient::<GameObservation>().downgrade();
        let user_ids: Vec<_> = self.watched.iter().cloned().collect();
        self.connections = user_ids
            .chunks(self.stream.users_per_connection())
            .map(|chunk| {
                Connection::open(
                    self.stream.clone(),
                    chunk.to_vec(),
                    self.settings.clone(),
                    observations.clone(),
                    self.connected.clone(),
                )
            })
            .collect();
        tracing::info!(
            users = self.watched.len(),
            connections = self.connections.len(),
            "watching lichess users"
        );
    }
}

impl Actor for GameMonitor {
    type Args = Self;
    type Error = Infallible;

    fn on_start(
        monitor: Self,
        _: ActorRef<Self>,
    ) -> impl Future<Output = Result<Self, Self::Error>> + Send {
        std::future::ready(Ok(monitor))
    }
}

impl Message<Watch> for GameMonitor {
    type Reply = ();

    async fn handle(&mut self, Watch(user_ids): Watch, ctx: &mut Context<Self, Self::Reply>) {
        if user_ids == self.watched {
            tracing::debug!(users = user_ids.len(), "watchlist unchanged");
            return;
        }
        self.watched = user_ids;
        self.reconnect(ctx.actor_ref());
    }
}

impl Message<GameObservation> for GameMonitor {
    type Reply = ();

    async fn handle(&mut self, observation: GameObservation, _: &mut Context<Self, Self::Reply>) {
        let Some(event) = observation.into_event() else {
            return;
        };
        tracing::info!(game = %event.game().id, kind = ?event.kind(), "game event observed");
        if self.listener.tell(event).await.is_err() {
            tracing::warn!("game event listener is not running; event dropped");
        }
    }
}

#[cfg(test)]
mod tests;
