use std::collections::BTreeSet;
use std::convert::Infallible;
use std::num::NonZeroUsize;

use kameo::Actor;
use kameo::actor::{ActorRef, Recipient};
use kameo::message::{Context, Message};
use tokio::sync::{broadcast, watch};

use crate::health::{Availability, Health, TokenOwner};
use crate::lichess;

use super::event::GameEvent;
use super::live::Connection;
use super::{StreamConnected, Watch};

const GAME_EVENTS_CAPACITY: usize = 1024;

#[derive(Debug)]
pub struct GameMonitor {
    lichess: lichess::Api,
    config: lichess::Config,
    events: broadcast::Sender<GameEvent>,
    connected: Recipient<StreamConnected>,
    health: watch::Receiver<Option<Health>>,
    token_owner: Option<lichess::UserId>,
    watched: BTreeSet<lichess::UserId>,
    users_per_connection: Option<NonZeroUsize>,
    connections: Vec<Connection>,
}

impl GameMonitor {
    #[must_use]
    pub fn new(
        lichess: lichess::Api,
        config: lichess::Config,
        connected: Recipient<StreamConnected>,
        health: watch::Receiver<Option<Health>>,
    ) -> Self {
        Self {
            lichess,
            config,
            events: broadcast::Sender::new(GAME_EVENTS_CAPACITY),
            connected,
            health,
            token_owner: None,
            watched: BTreeSet::new(),
            users_per_connection: None,
            connections: Vec::new(),
        }
    }

    #[must_use]
    pub fn events(&self) -> broadcast::Receiver<GameEvent> {
        self.events.subscribe()
    }

    fn publish(&self, event: GameEvent) {
        if self.events.send(event).is_err() {
            tracing::debug!("no one is subscribed to game events");
        }
    }

    fn reconnect(&mut self, monitor: &ActorRef<Self>, users_per_connection: NonZeroUsize) {
        self.connections.clear();
        let games = monitor
            .clone()
            .recipient::<lichess::GameState>()
            .downgrade();
        let user_ids: Vec<_> = self.watched.iter().cloned().collect();
        self.connections = user_ids
            .chunks(users_per_connection.get())
            .map(|chunk| {
                Connection::open(
                    self.lichess.clone(),
                    chunk.to_vec(),
                    self.config.stream.clone(),
                    games.clone(),
                    self.connected.clone(),
                )
            })
            .collect();
        tracing::info!(
            users = self.watched.len(),
            users_per_connection,
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
        if let Some(Health {
            lichess: Some(Availability::Available(TokenOwner(owner))),
            ..
        }) = &*self.health.borrow()
        {
            self.token_owner = Some(owner.clone());
        }
        let users_per_connection = self.config.users_per_connection(self.token_owner.as_ref());
        if user_ids == self.watched && Some(users_per_connection) == self.users_per_connection {
            tracing::debug!(users = user_ids.len(), "watchlist unchanged");
            return;
        }
        self.watched = user_ids;
        self.users_per_connection = Some(users_per_connection);
        self.reconnect(ctx.actor_ref(), users_per_connection);
    }
}

impl Message<lichess::GameState> for GameMonitor {
    type Reply = ();

    async fn handle(&mut self, state: lichess::GameState, _: &mut Context<Self, Self::Reply>) {
        let Some(event) = GameEvent::of(state) else {
            return;
        };
        tracing::info!(game = %event.game().id, kind = ?event.kind(), "game event observed");
        self.publish(event);
    }
}

#[cfg(test)]
mod tests;
