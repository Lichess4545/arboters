use std::collections::BTreeSet;
use std::time::Duration;

use kameo::actor::Recipient;
use reqwest::header::AUTHORIZATION;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use tokio::time::MissedTickBehavior;
use url::Url;

use crate::heltour::{ActiveLeagues, League, Pairing};

use super::Watch;

const FIND_PAIRING_PATH: &str = "find_pairing/";
const GET_LEAGUES_PATH: &str = "get_leagues/";

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
#[error("could not read the active leagues from heltour")]
pub struct ActiveLeaguesError(#[from] reqwest::Error);

#[derive(Deserialize)]
struct FindPairingAnswer {
    pairings: Option<Vec<Pairing>>,
}

#[derive(Deserialize)]
struct GetLeaguesAnswer {
    leagues: Vec<League>,
}

#[derive(Clone, Debug)]
pub struct Heltour {
    http: reqwest::Client,
    find_pairing: Url,
    get_leagues: Url,
    token: Option<SecretString>,
}

impl Heltour {
    pub fn new(http: reqwest::Client, settings: &HeltourSettings) -> Result<Self, url::ParseError> {
        Ok(Self {
            http,
            find_pairing: settings.base_url.join(FIND_PAIRING_PATH)?,
            get_leagues: settings.base_url.join(GET_LEAGUES_PATH)?,
            token: settings.token.clone(),
        })
    }

    async fn get<T: DeserializeOwned>(
        &self,
        endpoint: &Url,
        timeout: Duration,
    ) -> reqwest::Result<T> {
        let mut request = self.http.get(endpoint.clone()).timeout(timeout);
        if let Some(token) = &self.token {
            request = request.header(AUTHORIZATION, format!("Token {}", token.expose_secret()));
        }
        request.send().await?.error_for_status()?.json().await
    }

    async fn active_leagues(&self, timeout: Duration) -> Result<ActiveLeagues, ActiveLeaguesError> {
        let (leagues, pairings): (GetLeaguesAnswer, FindPairingAnswer) = tokio::try_join!(
            self.get(&self.get_leagues, timeout),
            self.get(&self.find_pairing, timeout),
        )?;
        Ok(ActiveLeagues {
            leagues: leagues.leagues,
            pairings: pairings.pairings.unwrap_or_default(),
        })
    }
}

pub async fn keep_in_sync(
    heltour: Heltour,
    monitor: Recipient<Watch>,
    followers: Vec<Recipient<ActiveLeagues>>,
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
        if monitor.tell(Watch(user_ids)).await.is_err() {
            tracing::info!("game monitor has stopped; ending watchlist sync");
            return;
        }
        for follower in &followers {
            if follower.tell(active.clone()).await.is_err() {
                tracing::info!(
                    "a follower of the active leagues has stopped; ending watchlist sync"
                );
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests;
