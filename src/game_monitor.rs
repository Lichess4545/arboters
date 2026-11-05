mod event;
mod live;
mod monitor;
mod phase;
pub mod watchlist;

use std::collections::BTreeSet;

use kameo::actor::Recipient;
use kameo::error::SendError;

use crate::lichess;

pub use event::{Ending, GameEvent, GameEventKind, GameOutcome};
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

#[derive(Debug)]
pub struct Follow<M: Send + 'static>(pub Recipient<M>);

#[derive(Debug, thiserror::Error)]
#[error("could not follow the game monitor")]
pub struct MonitorUnavailable(#[from] SendError<Follow<GameEvent>>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamConnected {
    pub players: usize,
}
