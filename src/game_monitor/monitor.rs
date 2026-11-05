use std::collections::BTreeSet;
use std::convert::Infallible;

use kameo::Actor;
use kameo::actor::{ActorRef, Recipient};
use kameo::message::{Context, Message};
use tokio::sync::watch;

use crate::health::{Availability, Health, TokenOwner};
use crate::lichess;

use super::event::GameEvent;
use super::live::Connection;
use super::{Follow, StreamConnected, Watch};

#[derive(Debug)]
pub struct GameMonitor {
    lichess: lichess::Api,
    config: lichess::Config,
    followers: Vec<Recipient<GameEvent>>,
    connected: Recipient<StreamConnected>,
    health: watch::Receiver<Option<Health>>,
    token_owner: Option<lichess::UserId>,
    watched: BTreeSet<lichess::UserId>,
    users_per_connection: usize,
    connections: Vec<Connection>,
}

impl GameMonitor {
    #[must_use]
    pub const fn new(
        lichess: lichess::Api,
        config: lichess::Config,
        connected: Recipient<StreamConnected>,
        health: watch::Receiver<Option<Health>>,
    ) -> Self {
        Self {
            lichess,
            config,
            followers: Vec::new(),
            connected,
            health,
            token_owner: None,
            watched: BTreeSet::new(),
            users_per_connection: 0,
            connections: Vec::new(),
        }
    }

    fn reconnect(&mut self, monitor: &ActorRef<Self>) {
        self.connections.clear();
        let games = monitor
            .clone()
            .recipient::<lichess::GameState>()
            .downgrade();
        let user_ids: Vec<_> = self.watched.iter().cloned().collect();
        self.connections = user_ids
            .chunks(self.users_per_connection)
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
            users_per_connection = self.users_per_connection,
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
        if user_ids == self.watched && users_per_connection == self.users_per_connection {
            tracing::debug!(users = user_ids.len(), "watchlist unchanged");
            return;
        }
        self.watched = user_ids;
        self.users_per_connection = users_per_connection;
        self.reconnect(ctx.actor_ref());
    }
}

impl Message<Follow<GameEvent>> for GameMonitor {
    type Reply = ();

    async fn handle(
        &mut self,
        Follow(follower): Follow<GameEvent>,
        _: &mut Context<Self, Self::Reply>,
    ) {
        self.followers.push(follower);
    }
}

impl Message<lichess::GameState> for GameMonitor {
    type Reply = ();

    async fn handle(&mut self, state: lichess::GameState, _: &mut Context<Self, Self::Reply>) {
        let Some(event) = GameEvent::of(state) else {
            return;
        };
        tracing::info!(game = %event.game().id, kind = ?event.kind(), "game event observed");
        let mut following = Vec::with_capacity(self.followers.len());
        for follower in std::mem::take(&mut self.followers) {
            if follower.tell(event.clone()).await.is_ok() {
                following.push(follower);
            } else {
                tracing::warn!(game = %event.game().id, "a game event follower has stopped; no longer following it");
            }
        }
        self.followers = following;
    }
}

#[cfg(test)]
mod tests;
