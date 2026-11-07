use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroUsize;
use std::time::Duration;

use kameo::Actor;
use kameo::actor::{ActorRef, Recipient, WeakActorRef};
use kameo::message::{Context, Message, StreamMessage};
use time::OffsetDateTime;
use tokio::sync::{broadcast, watch};
use tokio_stream::wrappers::WatchStream;

use crate::health::{Availability, Health, TokenOwner};
use crate::lichess;

use super::event::{GameChange, GameEvent, MatchedFixture};
use super::fixtures::{Fixtures, FixturesId, NewFixtures};
use super::live::Connection;
use super::{
    CurrentFixtures, FindCurrentFixtures, FixturesStore, League, PublishedEventsStore, Recorded,
    StoreError, StreamConnected, SubmitFixtures,
};

const GAME_EVENTS_CAPACITY: usize = 16_384;

#[derive(Debug)]
pub struct GameMonitor<F, P> {
    lichess: lichess::Api,
    config: lichess::Config,
    fixtures_store: F,
    published_events: P,
    active: BTreeMap<League, Fixtures>,
    active_fixtures: watch::Sender<Vec<Fixtures>>,
    events: broadcast::Sender<GameEvent>,
    connected: Recipient<StreamConnected>,
    token_owner: Option<lichess::UserId>,
    watched: BTreeSet<lichess::UserId>,
    users_per_connection: Option<NonZeroUsize>,
    connections: Vec<Connection>,
}

#[derive(Clone, Copy, Debug)]
struct FixturesEnded(FixturesId);

impl<F: FixturesStore, P: PublishedEventsStore> GameMonitor<F, P> {
    #[must_use]
    pub fn new(
        lichess: lichess::Api,
        config: lichess::Config,
        fixtures_store: F,
        published_events: P,
        connected: Recipient<StreamConnected>,
    ) -> Self {
        Self {
            lichess,
            config,
            fixtures_store,
            published_events,
            active: BTreeMap::new(),
            active_fixtures: watch::Sender::new(Vec::new()),
            events: broadcast::Sender::new(GAME_EVENTS_CAPACITY),
            connected,
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

    #[must_use]
    pub fn active_fixtures(&self) -> watch::Receiver<Vec<Fixtures>> {
        self.active_fixtures.subscribe()
    }

    async fn resume_stored_fixtures(&mut self, monitor: &ActorRef<Self>) -> Result<(), StoreError> {
        for fixtures in self.fixtures_store.active().await? {
            self.activate(fixtures, monitor);
        }
        Ok(())
    }

    fn unchanged(&self, new: &NewFixtures) -> Option<FixturesId> {
        self.active
            .get(&new.league)
            .filter(|current| current.has_the_games_and_period_of(new))
            .map(|current| current.id)
    }

    async fn store(
        &mut self,
        new: NewFixtures,
        monitor: &ActorRef<Self>,
    ) -> Result<FixturesId, StoreError> {
        let fixtures = self.fixtures_store.store(new).await?;
        let id = fixtures.id;
        tracing::info!(fixtures = %id, league = %fixtures.league, games = fixtures.games.len(), "fixtures stored");
        self.activate(fixtures, monitor);
        self.watch_active_players(monitor);
        Ok(id)
    }

    fn activate(&mut self, fixtures: Fixtures, monitor: &ActorRef<Self>) {
        complete_when_ended(&fixtures, monitor.downgrade());
        self.active.insert(fixtures.league.clone(), fixtures);
        self.publish_active_fixtures();
    }

    fn publish_active_fixtures(&self) {
        self.active_fixtures
            .send_replace(self.active.values().cloned().collect());
    }

    async fn complete(&mut self, id: FixturesId, monitor: &ActorRef<Self>) {
        let Some(league) = self.league_of_active(id) else {
            return;
        };
        if let Err(error) = self.fixtures_store.complete(id).await {
            tracing::error!(error = %error, cause = ?std::error::Error::source(&error), fixtures = %id, "could not mark the fixtures complete");
        }
        self.active.remove(&league);
        self.publish_active_fixtures();
        tracing::info!(fixtures = %id, league = %league, "fixtures complete");
        self.watch_active_players(monitor);
    }

    fn league_of_active(&self, id: FixturesId) -> Option<League> {
        self.active
            .values()
            .find(|fixtures| fixtures.id == id)
            .map(|fixtures| fixtures.league.clone())
    }

    fn current_fixtures_of(&self, leagues: &BTreeSet<League>) -> CurrentFixtures {
        CurrentFixtures(
            self.active
                .iter()
                .filter(|(league, _)| leagues.contains(*league))
                .map(|(league, fixtures)| (league.clone(), fixtures.id))
                .collect(),
        )
    }

    fn fixtures_played_in(&self, game: &lichess::Game) -> Vec<MatchedFixture> {
        self.active
            .values()
            .flat_map(|fixtures| fixtures.played_in(game))
            .collect()
    }

    fn note_token_owner(&mut self, health: Option<&Health>) {
        if let Some(Health {
            lichess: Some(Availability::Available(TokenOwner(owner))),
            ..
        }) = health
        {
            self.token_owner = Some(owner.clone());
        }
    }

    fn watch_active_players(&mut self, monitor: &ActorRef<Self>) {
        let players: BTreeSet<_> = self
            .active
            .values()
            .flat_map(Fixtures::players)
            .cloned()
            .collect();
        let users_per_connection = self.config.users_per_connection(self.token_owner.as_ref());
        if players == self.watched && Some(users_per_connection) == self.users_per_connection {
            tracing::debug!(users = players.len(), "watched players unchanged");
            return;
        }
        self.watched = players;
        self.users_per_connection = Some(users_per_connection);
        self.reconnect(monitor, users_per_connection);
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

    async fn publish_once(&self, event: GameEvent) {
        match self.published_events.record(&event).await {
            Ok(Recorded::First) => self.publish(event),
            Ok(Recorded::Again) => {
                tracing::debug!(game = %event.game().id, kind = ?event.kind(), "game event already published");
            }
            Err(error) => {
                tracing::error!(error = %error, cause = ?std::error::Error::source(&error), game = %event.game().id, kind = ?event.kind(), "could not record the game event, so it is not published");
            }
        }
    }

    fn publish(&self, event: GameEvent) {
        let fixtures: Vec<_> = event
            .matched
            .iter()
            .map(|matched| matched.fixtures_id)
            .collect();
        tracing::info!(
            game = %event.game().id,
            kind = ?event.kind(),
            ?fixtures,
            leagues = ?event.leagues(),
            "game event published"
        );
        if self.events.send(event).is_err() {
            tracing::debug!("no one is subscribed to game events");
        }
    }
}

fn complete_when_ended<F: FixturesStore, P: PublishedEventsStore>(
    fixtures: &Fixtures,
    monitor: WeakActorRef<GameMonitor<F, P>>,
) {
    let Some(ends) = fixtures.period.ends else {
        return;
    };
    let id = fixtures.id;
    let remaining = Duration::try_from(ends - OffsetDateTime::now_utc()).unwrap_or_default();
    tokio::spawn(async move {
        tokio::time::sleep(remaining).await;
        if let Some(monitor) = monitor.upgrade()
            && monitor.tell(FixturesEnded(id)).await.is_err()
        {
            tracing::debug!(fixtures = %id, "game monitor stopped before the fixtures ended");
        }
    });
}

impl<F: FixturesStore, P: PublishedEventsStore> Actor for GameMonitor<F, P> {
    type Args = (Self, watch::Receiver<Option<Health>>);
    type Error = StoreError;

    async fn on_start(
        (mut monitor, health): Self::Args,
        monitor_ref: ActorRef<Self>,
    ) -> Result<Self, Self::Error> {
        monitor.note_token_owner(health.borrow().as_ref());
        monitor.resume_stored_fixtures(&monitor_ref).await?;
        monitor_ref.attach_stream(WatchStream::new(health), (), ());
        Ok(monitor)
    }
}

impl<F: FixturesStore, P: PublishedEventsStore> Message<SubmitFixtures> for GameMonitor<F, P> {
    type Reply = ();

    async fn handle(
        &mut self,
        SubmitFixtures { fixtures, stored }: SubmitFixtures,
        ctx: &mut Context<Self, Self::Reply>,
    ) {
        let league = fixtures.league.clone();
        let id = match self.unchanged(&fixtures) {
            Some(id) => id,
            None => match self.store(fixtures, ctx.actor_ref()).await {
                Ok(id) => id,
                Err(error) => {
                    tracing::error!(error = %error, cause = ?std::error::Error::source(&error), league = %league, "could not store the fixtures");
                    return;
                }
            },
        };
        if stored.send(id).is_err() {
            tracing::debug!(fixtures = %id, league = %league, "no one waits for the stored fixtures");
        }
    }
}

impl<F: FixturesStore, P: PublishedEventsStore> Message<FindCurrentFixtures> for GameMonitor<F, P> {
    type Reply = ();

    fn handle(
        &mut self,
        FindCurrentFixtures { leagues, found }: FindCurrentFixtures,
        _: &mut Context<Self, Self::Reply>,
    ) -> impl Future<Output = Self::Reply> + Send {
        if found.send(self.current_fixtures_of(&leagues)).is_err() {
            tracing::debug!("no one waits for the current fixtures");
        }
        std::future::ready(())
    }
}

impl<F: FixturesStore, P: PublishedEventsStore> Message<FixturesEnded> for GameMonitor<F, P> {
    type Reply = ();

    async fn handle(
        &mut self,
        FixturesEnded(id): FixturesEnded,
        ctx: &mut Context<Self, Self::Reply>,
    ) {
        self.complete(id, ctx.actor_ref()).await;
    }
}

impl<F: FixturesStore, P: PublishedEventsStore> Message<StreamMessage<Option<Health>, (), ()>>
    for GameMonitor<F, P>
{
    type Reply = ();

    fn handle(
        &mut self,
        health: StreamMessage<Option<Health>, (), ()>,
        ctx: &mut Context<Self, Self::Reply>,
    ) -> impl Future<Output = Self::Reply> + Send {
        if let StreamMessage::Next(health) = health {
            self.note_token_owner(health.as_ref());
            self.watch_active_players(ctx.actor_ref());
        }
        std::future::ready(())
    }
}

impl<F: FixturesStore, P: PublishedEventsStore> Message<lichess::GameState> for GameMonitor<F, P> {
    type Reply = ();

    async fn handle(&mut self, state: lichess::GameState, _: &mut Context<Self, Self::Reply>) {
        let Some(change) = GameChange::of(state) else {
            return;
        };
        let matched = self.fixtures_played_in(change.game());
        self.publish_once(GameEvent { change, matched }).await;
    }
}

#[cfg(test)]
mod tests;
