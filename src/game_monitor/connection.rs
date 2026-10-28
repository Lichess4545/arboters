use backon::{BackoffBuilder, ExponentialBackoff, ExponentialBuilder};
use kameo::actor::{Recipient, WeakRecipient};
use tokio::task::JoinHandle;

use crate::lichess;

use super::StreamConnected;
use super::settings::StreamSettings;
use super::stream::{FeedError, FeedEvent, GameObservation, LichessFeed, LichessStream};

#[derive(Debug)]
pub struct Connection {
    task: JoinHandle<()>,
}

impl Connection {
    pub fn open(
        stream: LichessStream,
        user_ids: Vec<lichess::UserId>,
        settings: StreamSettings,
        observations: WeakRecipient<GameObservation>,
        connected: Recipient<StreamConnected>,
    ) -> Self {
        Self {
            task: tokio::spawn(supervise(
                stream,
                user_ids,
                settings,
                observations,
                connected,
            )),
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
    stream: LichessStream,
    user_ids: Vec<lichess::UserId>,
    settings: StreamSettings,
    observations: WeakRecipient<GameObservation>,
    connected: Recipient<StreamConnected>,
) {
    let reconnect_delays = ExponentialBuilder::new()
        .with_min_delay(settings.initial_backoff)
        .with_max_delay(settings.max_backoff)
        .without_max_times();
    let mut upcoming_delays = reconnect_delays.build();
    loop {
        let failure = match stream.connect(&user_ids).await {
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
                match relay(feed, &observations, reconnect_delays, &mut upcoming_delays).await {
                    Relay::ObserverGone => return,
                    Relay::Ended => None,
                    Relay::Failed(error) => Some(error),
                }
            }
            Err(error) => Some(error),
        };
        let delay = upcoming_delays.next().unwrap_or(settings.max_backoff);
        let delay = match failure {
            Some(FeedError::RateLimited) => delay.max(settings.rate_limit_backoff),
            _ => delay,
        };
        if let Some(error) = &failure {
            tracing::warn!(error = ?error, retry_in = ?delay, users = user_ids.len(), "game feed failed");
        } else {
            tracing::info!(retry_in = ?delay, users = user_ids.len(), "game feed ended");
        }
        tokio::time::sleep(delay).await;
    }
}

async fn relay(
    mut feed: LichessFeed,
    observations: &WeakRecipient<GameObservation>,
    reconnect_delays: ExponentialBuilder,
    upcoming_delays: &mut ExponentialBackoff,
) -> Relay {
    loop {
        match feed.next_event().await {
            None => return Relay::Ended,
            Some(Err(error)) => return Relay::Failed(error),
            Some(Ok(FeedEvent::Heartbeat)) => *upcoming_delays = reconnect_delays.build(),
            Some(Ok(FeedEvent::Observed(observation))) => {
                *upcoming_delays = reconnect_delays.build();
                let Some(observer) = observations.upgrade() else {
                    return Relay::ObserverGone;
                };
                if observer.tell(observation).await.is_err() {
                    return Relay::ObserverGone;
                }
            }
        }
    }
}
