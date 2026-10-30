use std::collections::HashMap;
use std::convert::Infallible;
use std::time::Duration;

use kameo::Actor;
use kameo::actor::ActorRef;
use kameo::message::{Context, Message};
use reqwest::header::CONTENT_TYPE;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use tokio::sync::watch;
use tokio::time::MissedTickBehavior;
use url::Url;

use crate::lichess;

use super::settings::LichessSettings;

const TOKEN_TEST_PATH: &str = "api/token/test";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TokenState {
    Untested,
    Valid { owner: lichess::UserId },
    Invalid,
}

#[derive(Debug, thiserror::Error)]
#[error("could not test the lichess token")]
struct TokenTestError(#[from] reqwest::Error);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TokenOwner {
    user_id: lichess::UserId,
}

#[derive(Debug)]
pub struct TokenCheck {
    http: reqwest::Client,
    endpoint: Url,
    token: SecretString,
    interval: Duration,
    state: watch::Sender<TokenState>,
}

impl TokenCheck {
    pub fn new(
        http: reqwest::Client,
        settings: &LichessSettings,
        token: SecretString,
    ) -> Result<(Self, watch::Receiver<TokenState>), url::ParseError> {
        let (state, observed) = watch::channel(TokenState::Untested);
        let check = Self {
            http,
            endpoint: settings.base_url.join(TOKEN_TEST_PATH)?,
            token,
            interval: settings.token_check_interval,
            state,
        };
        Ok((check, observed))
    }

    async fn test(&self) -> Result<TokenState, TokenTestError> {
        let owners: HashMap<String, Option<TokenOwner>> = self
            .http
            .post(self.endpoint.clone())
            .header(CONTENT_TYPE, "text/plain")
            .body(self.token.expose_secret().to_owned())
            .timeout(self.interval)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(if let Some(owner) = owners.into_values().flatten().next() {
            TokenState::Valid {
                owner: owner.user_id,
            }
        } else {
            tracing::error!("lichess reports the token invalid");
            TokenState::Invalid
        })
    }
}

impl Actor for TokenCheck {
    type Args = Self;
    type Error = Infallible;

    fn on_start(
        check: Self,
        check_ref: ActorRef<Self>,
    ) -> impl Future<Output = Result<Self, Self::Error>> + Send {
        let mut checks = tokio::time::interval(check.interval);
        checks.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let check_ref = check_ref.downgrade();
        tokio::spawn(async move {
            loop {
                checks.tick().await;
                let Some(check) = check_ref.upgrade() else {
                    return;
                };
                if check.tell(Check).await.is_err() {
                    return;
                }
            }
        });
        std::future::ready(Ok(check))
    }
}

#[derive(Debug)]
struct Check;

impl Message<Check> for TokenCheck {
    type Reply = ();

    async fn handle(&mut self, _: Check, _: &mut Context<Self, Self::Reply>) {
        match self.test().await {
            Ok(state) => {
                tracing::debug!(state = ?state, "lichess token tested");
                self.state.send_replace(state);
            }
            Err(error) => {
                tracing::warn!(error = ?error, "keeping the previous lichess token state");
            }
        }
    }
}

#[cfg(test)]
mod tests;
