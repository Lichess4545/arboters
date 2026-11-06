mod event;
mod fixtures;
pub mod league;
mod live;
mod monitor;
mod stores;

use std::collections::{BTreeMap, BTreeSet};

use futures::Stream;
use tokio::sync::{broadcast, oneshot};
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;

pub use event::{
    Decision, Draw, GameChange, GameEvent, GameEventKind, MatchedFixture, VoidReason, Win,
};
pub use fixtures::{Difference, Fixture, Fixtures, FixturesId, NewFixtures, Period};
pub use league::League;
pub use monitor::GameMonitor;
pub use stores::{
    FixturesStore, PostgresFixturesStore, PostgresPublishedEventsStore, PublishedEventsStore,
    Recorded, StoreError,
};

const USER_AGENT: &str = concat!(
    env!("CARGO_PKG_NAME"),
    "/",
    env!("CARGO_PKG_VERSION"),
    " (+",
    env!("CARGO_PKG_REPOSITORY"),
    ")"
);

pub fn http_client() -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder().user_agent(USER_AGENT).build()
}

#[derive(Debug)]
pub struct SubmitFixtures {
    pub fixtures: NewFixtures,
    pub stored: oneshot::Sender<FixturesId>,
}

#[derive(Debug)]
pub struct FindCurrentFixtures {
    pub leagues: BTreeSet<League>,
    pub found: oneshot::Sender<CurrentFixtures>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CurrentFixtures(pub BTreeMap<League, FixturesId>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamConnected {
    pub players: usize,
}

pub fn event_stream(
    events: broadcast::Receiver<GameEvent>,
) -> impl Stream<Item = GameEvent> + Send + Unpin + 'static {
    BroadcastStream::new(events).filter_map(|received| match received {
        Ok(event) => Some(event),
        Err(BroadcastStreamRecvError::Lagged(skipped)) => {
            tracing::warn!(skipped, "fell behind the game events; skipped some");
            None
        }
    })
}

#[cfg(test)]
mod tests;
