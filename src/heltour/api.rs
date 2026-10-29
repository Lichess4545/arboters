use std::time::Duration;

use reqwest::RequestBuilder;
use reqwest::header::AUTHORIZATION;
use secrecy::{ExposeSecret, SecretString};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::lichess;

use super::active_leagues::ActiveLeagues;
use super::league::{self, League};
use super::pairing::Pairing;
use super::team_pairing::TeamPairing;

const FIND_PAIRING_PATH: &str = "find_pairing/";
const GET_LEAGUES_PATH: &str = "get_leagues/";
const GET_TEAM_PAIRINGS_PATH: &str = "get_team_pairings/";
const UPDATE_PAIRING_PATH: &str = "update_pairing/";
const UPDATE_PAIRING_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeltourSettings {
    pub base_url: Url,
    pub token: Option<SecretString>,
}

#[derive(Debug, thiserror::Error)]
#[error("could not read the active leagues from heltour")]
pub struct ActiveLeaguesError(#[from] reqwest::Error);

#[derive(Debug, thiserror::Error)]
#[error("could not read the team pairings from heltour")]
pub struct TeamPairingsError(#[from] reqwest::Error);

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
    pub game: lichess::GameId,
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
    #[error("heltour refused to update the pairing: {0:?}")]
    Refused(Refusal),
    #[error("could not ask heltour to update the pairing")]
    Request(#[from] reqwest::Error),
}

#[derive(Serialize)]
struct UpdatePairingForm<'a> {
    league: &'a str,
    white: &'a str,
    black: &'a str,
    game_link: String,
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
struct GetTeamPairingsAnswer {
    team_pairings: Vec<TeamPairing>,
}

#[derive(Deserialize)]
struct UpdatePairingAnswer {
    error: Option<Refusal>,
}

#[derive(Clone, Debug)]
pub struct Heltour {
    http: reqwest::Client,
    find_pairing: Url,
    get_leagues: Url,
    get_team_pairings: Url,
    update_pairing: Url,
    token: Option<SecretString>,
}

impl Heltour {
    pub fn new(http: reqwest::Client, settings: &HeltourSettings) -> Result<Self, url::ParseError> {
        Ok(Self {
            http,
            find_pairing: settings.base_url.join(FIND_PAIRING_PATH)?,
            get_leagues: settings.base_url.join(GET_LEAGUES_PATH)?,
            get_team_pairings: settings.base_url.join(GET_TEAM_PAIRINGS_PATH)?,
            update_pairing: settings.base_url.join(UPDATE_PAIRING_PATH)?,
            token: settings.token.clone(),
        })
    }

    async fn answer<T: DeserializeOwned>(&self, request: RequestBuilder) -> reqwest::Result<T> {
        let request = match &self.token {
            Some(token) => {
                request.header(AUTHORIZATION, format!("Token {}", token.expose_secret()))
            }
            None => request,
        };
        request.send().await?.error_for_status()?.json().await
    }

    pub async fn active_leagues(
        &self,
        timeout: Duration,
    ) -> Result<ActiveLeagues, ActiveLeaguesError> {
        let get = |endpoint: &Url| self.http.get(endpoint.clone()).timeout(timeout);
        let (leagues, pairings): (GetLeaguesAnswer, FindPairingAnswer) = tokio::try_join!(
            self.answer(get(&self.get_leagues)),
            self.answer(get(&self.find_pairing)),
        )?;
        Ok(ActiveLeagues {
            leagues: leagues.leagues,
            pairings: pairings.pairings.unwrap_or_default(),
            team_pairings: Vec::new(),
        })
    }

    pub async fn team_pairings(
        &self,
        timeout: Duration,
    ) -> Result<Vec<TeamPairing>, TeamPairingsError> {
        let request = self
            .http
            .get(self.get_team_pairings.clone())
            .timeout(timeout);
        let answer: GetTeamPairingsAnswer = self.answer(request).await?;
        Ok(answer.team_pairings)
    }

    pub async fn update_pairing(&self, update: &PairingUpdate) -> Result<(), UpdatePairingError> {
        let form = UpdatePairingForm {
            league: update.league.as_str(),
            white: update.white.as_str(),
            black: update.black.as_str(),
            game_link: update.game.link(),
            result: update.result,
            strict: true,
        };
        let request = self
            .http
            .post(self.update_pairing.clone())
            .form(&form)
            .timeout(UPDATE_PAIRING_TIMEOUT);
        let answer: UpdatePairingAnswer = self.answer(request).await?;
        answer
            .error
            .map_or(Ok(()), |refusal| Err(UpdatePairingError::Refused(refusal)))
    }
}
