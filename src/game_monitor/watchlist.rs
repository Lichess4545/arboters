use std::collections::BTreeSet;
use std::time::Duration;

use kameo::actor::Recipient;
use reqwest::header::AUTHORIZATION;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use tokio::time::MissedTickBehavior;
use url::Url;

use crate::heltour::{CurrentPairings, Pairing};

use super::Watch;

const FIND_PAIRING_PATH: &str = "find_pairing/";

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeltourSettings {
    pub base_url: Url,
    pub token: Option<SecretString>,
}

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

#[derive(Debug, thiserror::Error)]
#[error("could not load the current pairings from heltour")]
pub struct PairingsError(#[from] reqwest::Error);

#[derive(Deserialize)]
struct FindPairingAnswer {
    pairings: Option<Vec<Pairing>>,
}

#[derive(Clone, Debug)]
pub struct Heltour {
    http: reqwest::Client,
    find_pairing: Url,
    token: Option<SecretString>,
}

impl Heltour {
    pub fn new(http: reqwest::Client, settings: &HeltourSettings) -> Result<Self, url::ParseError> {
        Ok(Self {
            http,
            find_pairing: settings.base_url.join(FIND_PAIRING_PATH)?,
            token: settings.token.clone(),
        })
    }

    async fn pairings(&self, timeout: Duration) -> Result<Vec<Pairing>, PairingsError> {
        let mut request = self.http.get(self.find_pairing.clone()).timeout(timeout);
        if let Some(token) = &self.token {
            request = request.header(AUTHORIZATION, format!("Token {}", token.expose_secret()));
        }
        let answer: FindPairingAnswer = request.send().await?.error_for_status()?.json().await?;
        Ok(answer.pairings.unwrap_or_default())
    }
}

pub async fn keep_in_sync(
    heltour: Heltour,
    monitor: Recipient<Watch>,
    notifier: Recipient<CurrentPairings>,
    refresh_interval: Duration,
) {
    let mut refreshes = tokio::time::interval(refresh_interval);
    refreshes.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        refreshes.tick().await;
        let pairings = match heltour.pairings(refresh_interval).await {
            Ok(pairings) => pairings,
            Err(error) => {
                tracing::warn!(error = ?error, "keeping previous watchlist");
                continue;
            }
        };
        let user_ids: BTreeSet<_> = pairings
            .iter()
            .flat_map(|pairing| [pairing.white.clone(), pairing.black.clone()])
            .collect();
        tracing::debug!(
            players = user_ids.len(),
            "refreshing game monitor watchlist"
        );
        if monitor.tell(Watch(user_ids)).await.is_err()
            || notifier.tell(CurrentPairings(pairings)).await.is_err()
        {
            tracing::info!("a follower of the watchlist has stopped; ending watchlist sync");
            return;
        }
    }
}

#[cfg(test)]
mod tests;
