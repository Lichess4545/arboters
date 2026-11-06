mod config;

use std::collections::BTreeSet;
use std::time::Duration;

use kameo::actor::Recipient;
use tokio::sync::watch;
use tokio::time::MissedTickBehavior;

use crate::heltour::{ActiveLeagues, Heltour};

use super::Watch;

pub use config::Config;

pub async fn keep_in_sync(
    heltour: Heltour,
    monitor: Recipient<Watch>,
    active_leagues: watch::Sender<ActiveLeagues>,
    refresh_interval: Duration,
) {
    let mut refreshes = tokio::time::interval(refresh_interval);
    refreshes.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        refreshes.tick().await;
        let active = match heltour.active_leagues(refresh_interval).await {
            Ok(active) => active,
            Err(error) => {
                tracing::warn!(error = ?error, "keeping previous watchlist");
                continue;
            }
        };
        let user_ids: BTreeSet<_> = active
            .pairings
            .iter()
            .flat_map(|pairing| [pairing.white.clone(), pairing.black.clone()])
            .collect();
        tracing::debug!(
            leagues = active.leagues.len(),
            players = user_ids.len(),
            "refreshing game monitor watchlist"
        );
        active_leagues.send_replace(active);
        if monitor.tell(Watch(user_ids)).await.is_err() {
            tracing::info!("game monitor has stopped; ending watchlist sync");
            return;
        }
    }
}

#[cfg(test)]
mod tests;
