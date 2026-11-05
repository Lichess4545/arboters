use backon::{BackoffBuilder, ExponentialBackoff, ExponentialBuilder};
use kameo::actor::{Recipient, WeakRecipient};
use tokio::task::JoinHandle;

use crate::lichess::{self, FeedError, FeedEvent, GameFeed, GameState};

use super::StreamConnected;

#[derive(Debug)]
pub struct Connection {
    task: JoinHandle<()>,
}

impl Connection {
    pub fn open(
        lichess: lichess::Api,
        user_ids: Vec<lichess::UserId>,
        config: lichess::StreamConfig,
        games: WeakRecipient<GameState>,
        connected: Recipient<StreamConnected>,
    ) -> Self {
        Self {
            task: tokio::spawn(supervise(lichess, user_ids, config, games, connected)),
        }
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.task.abort();
    }
}

enum Relay {
    Ended,
    Failed(FeedError),
    ObserverGone,
}

async fn supervise(
    lichess: lichess::Api,
    user_ids: Vec<lichess::UserId>,
    config: lichess::StreamConfig,
    games: WeakRecipient<GameState>,
    connected: Recipient<StreamConnected>,
) {
    let reconnect_delays = ExponentialBuilder::new()
        .with_min_delay(config.initial_backoff)
        .with_max_delay(config.max_backoff)
        .without_max_times();
    let mut upcoming_delays = reconnect_delays.build();
    loop {
        let failure = match lichess
            .stream_games_by_users(&user_ids, config.inactivity_timeout, config.max_line_bytes)
            .await
        {
            Ok(feed) => {
                tracing::info!(users = user_ids.len(), "game feed connected");
                let report = StreamConnected {
                    players: user_ids.len(),
                };
                if connected.tell(report).await.is_err() {
                    tracing::warn!(
                        users = user_ids.len(),
                        "stream connection listener is not running"
                    );
                }
                match relay(feed, &games, reconnect_delays, &mut upcoming_delays).await {
                    Relay::ObserverGone => return,
                    Relay::Ended => None,
                    Relay::Failed(error) => Some(error),
                }
            }
            Err(error) => Some(error),
        };
        let delay = upcoming_delays.next().unwrap_or(config.max_backoff);
        let delay = match failure {
            Some(FeedError::RateLimited) => delay.max(config.rate_limit_backoff),
            _ => delay,
        };
        match &failure {
            Some(error @ (FeedError::Undecodable { .. } | FeedError::LineTooLong { .. })) => {
                tracing::error!(error = ?error, retry_in = ?delay, users = user_ids.len(), "could not read the game feed");
            }
            Some(error) => {
                tracing::warn!(error = ?error, retry_in = ?delay, users = user_ids.len(), "game feed failed");
            }
            None => {
                tracing::info!(retry_in = ?delay, users = user_ids.len(), "game feed ended");
            }
        }
        tokio::time::sleep(delay).await;
    }
}

async fn relay(
    mut feed: GameFeed,
    games: &WeakRecipient<GameState>,
    reconnect_delays: ExponentialBuilder,
    upcoming_delays: &mut ExponentialBackoff,
) -> Relay {
    loop {
        match feed.next_event().await {
            None => return Relay::Ended,
            Some(Err(error)) => return Relay::Failed(error),
            Some(Ok(FeedEvent::Heartbeat)) => *upcoming_delays = reconnect_delays.build(),
            Some(Ok(FeedEvent::Game(state))) => {
                *upcoming_delays = reconnect_delays.build();
                let Some(observer) = games.upgrade() else {
                    return Relay::ObserverGone;
                };
                if observer.tell(state).await.is_err() {
                    return Relay::ObserverGone;
                }
            }
        }
    }
}
