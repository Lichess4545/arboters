mod event;
mod live;
mod monitor;
pub mod watchlist;

use std::collections::BTreeSet;

use futures::Stream;
use tokio::sync::broadcast;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;

use crate::lichess;

pub use event::{Decision, Draw, GameEvent, GameEventKind, VoidReason, Win};
pub use monitor::GameMonitor;

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Watch(pub BTreeSet<lichess::UserId>);

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
