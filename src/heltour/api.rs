use std::time::Duration;

use reqwest::header::AUTHORIZATION;
use reqwest::{Method, RequestBuilder, Response};
use secrecy::{ExposeSecret, SecretString};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::lichess;

use super::active_leagues::ActiveLeagues;
use super::league::{self, League};
use super::pairing::Pairing;

const FIND_PAIRING_PATH: &str = "find_pairing/";
const GET_LEAGUES_PATH: &str = "get_leagues/";
const TOKEN_TEST_PATH: &str = "v2/token/test/";
const UPDATE_PAIRING_PATH: &str = "update_pairing/";
const UPDATE_PAIRING_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeltourSettings {
    pub base_url: Url,
    pub token: SecretString,
}

#[derive(Debug, thiserror::Error)]
#[error("could not read the active leagues from Heltour")]
pub struct ActiveLeaguesError(#[from] reqwest::Error);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum PairingResult {
    #[serde(rename = "1-0")]
    WhiteWon,
    #[serde(rename = "1/2-1/2")]
    Draw,
    #[serde(rename = "0-1")]
    BlackWon,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PairingUpdate {
    pub league: league::Tag,
    pub white: lichess::UserId,
    pub black: lichess::UserId,
    pub game_link: Url,
    pub result: Option<PairingResult>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Refusal {
    NoMatchingRounds,
    NotFound,
    Ambiguous,
    GameLinkConflict,
    ResultAlreadySet,
}

#[derive(Debug, thiserror::Error)]
pub enum UpdatePairingError {
    #[error("Heltour refused to update the pairing: {0:?}")]
    Refused(Refusal),
    #[error("could not ask Heltour to update the pairing")]
    Request(#[from] reqwest::Error),
}

#[derive(Serialize)]
struct UpdatePairingForm<'a> {
    league: &'a str,
    white: &'a str,
    black: &'a str,
    game_link: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<PairingResult>,
    strict: bool,
}

#[derive(Deserialize)]
struct FindPairingAnswer {
    pairings: Option<Vec<Pairing>>,
}

#[derive(Deserialize)]
struct GetLeaguesAnswer {
    leagues: Vec<League>,
}

#[derive(Deserialize)]
struct UpdatePairingAnswer {
    error: Option<Refusal>,
}

#[derive(Clone, Debug)]
pub struct Heltour {
    http: reqwest::Client,
    base_url: Url,
    token: SecretString,
}

impl Heltour {
    #[must_use]
    pub fn new(http: reqwest::Client, settings: &HeltourSettings) -> Self {
        Self {
            http,
            base_url: settings.base_url.clone(),
            token: settings.token.clone(),
        }
    }

    #[expect(clippy::expect_used)]
    fn request(&self, method: Method, path: &str) -> RequestBuilder {
        self.http
            .request(method, self.base_url.join(path).expect("constant path"))
            .header(
                AUTHORIZATION,
                format!("Token {}", self.token.expose_secret()),
            )
    }

    pub async fn test_token(&self) -> reqwest::Result<()> {
        send(self.request(Method::GET, TOKEN_TEST_PATH))
            .await
            .map(drop)
    }

    pub async fn active_leagues(
        &self,
        timeout: Duration,
    ) -> Result<ActiveLeagues, ActiveLeaguesError> {
        let get = |path| self.request(Method::GET, path).timeout(timeout);
        let (leagues, pairings): (GetLeaguesAnswer, FindPairingAnswer) = tokio::try_join!(
            answer(get(GET_LEAGUES_PATH)),
            answer(get(FIND_PAIRING_PATH)),
        )?;
        Ok(ActiveLeagues {
            leagues: leagues.leagues,
            pairings: pairings.pairings.unwrap_or_default(),
        })
    }

    pub async fn update_pairing(&self, update: &PairingUpdate) -> Result<(), UpdatePairingError> {
        let form = UpdatePairingForm {
            league: update.league.as_str(),
            white: update.white.as_str(),
            black: update.black.as_str(),
            game_link: update.game_link.as_str(),
            result: update.result,
            strict: true,
        };
        let request = self
            .request(Method::POST, UPDATE_PAIRING_PATH)
            .form(&form)
            .timeout(UPDATE_PAIRING_TIMEOUT);
        let answer: UpdatePairingAnswer = answer(request).await?;
        answer
            .error
            .map_or(Ok(()), |refusal| Err(UpdatePairingError::Refused(refusal)))
    }
}

async fn send(request: RequestBuilder) -> reqwest::Result<Response> {
    request.send().await?.error_for_status()
}

async fn answer<T: DeserializeOwned>(request: RequestBuilder) -> reqwest::Result<T> {
    send(request).await?.json().await
}
