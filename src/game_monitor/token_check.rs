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
const LICHESS4545: &str = "lichess4545";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenHealth {
    Usable,
    Unusable,
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
    health: watch::Sender<TokenHealth>,
}

impl TokenCheck {
    pub fn new(
        http: reqwest::Client,
        settings: &LichessSettings,
        token: SecretString,
    ) -> Result<(Self, watch::Receiver<TokenHealth>), url::ParseError> {
        let (health, observed) = watch::channel(TokenHealth::Usable);
        let check = Self {
            http,
            endpoint: settings.base_url.join(TOKEN_TEST_PATH)?,
            token,
            interval: settings.token_check_interval,
            health,
        };
        Ok((check, observed))
    }

    async fn test(&self) -> Result<TokenHealth, TokenTestError> {
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
        Ok(match owners.into_values().next().flatten() {
            None => {
                tracing::error!("lichess reports the token invalid");
                TokenHealth::Unusable
            }
            Some(owner) if owner.user_id.as_str() == LICHESS4545 => TokenHealth::Usable,
            Some(owner) => {
                tracing::error!(owner = %owner.user_id, expected = LICHESS4545, "the lichess token belongs to another account");
                TokenHealth::Unusable
            }
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
            Ok(health) => {
                tracing::debug!(health = ?health, "lichess token tested");
                self.health.send_replace(health);
            }
            Err(error) => {
                tracing::warn!(error = ?error, "keeping the previous lichess token health");
            }
        }
    }
}

#[cfg(test)]
mod tests;
