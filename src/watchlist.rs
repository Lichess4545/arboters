use std::time::Duration;

use kameo::actor::Recipient;
use serde::Deserialize;
use tokio::time::MissedTickBehavior;

use crate::game_monitor::Watch;
use crate::subscriptions::SubscriptionRepository;

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WatchlistSettings {
    #[serde(with = "humantime_serde")]
    pub refresh_interval: Duration,
}

impl Default for WatchlistSettings {
    fn default() -> Self {
        Self {
            refresh_interval: Duration::from_mins(5),
        }
    }
}

pub async fn keep_in_sync<R: SubscriptionRepository>(
    subscriptions: R,
    monitor: Recipient<Watch>,
    refresh_interval: Duration,
) {
    let mut refreshes = tokio::time::interval(refresh_interval);
    refreshes.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        refreshes.tick().await;
        let usernames = match subscriptions.watched_usernames().await {
            Ok(usernames) => usernames,
            Err(error) => {
                tracing::warn!(error = %error, "could not load watched usernames; keeping previous watchlist");
                continue;
            }
        };
        tracing::debug!(count = usernames.len(), "refreshing game monitor watchlist");
        if monitor.tell(Watch(usernames)).await.is_err() {
            tracing::info!("game monitor has stopped; ending watchlist sync");
            return;
        }
    }
}
