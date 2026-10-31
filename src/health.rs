mod checks;
mod config;

use std::convert::Infallible;
use std::time::Duration;

use backon::{ExponentialBuilder, Retryable};
use futures::future::OptionFuture;
use kameo::Actor;
use kameo::actor::{ActorRef, WeakActorRef};
use kameo::error::ActorStopReason;
use kameo::message::{Context, Message};
use sqlx::PgPool;
use tokio::sync::watch;
use tokio::time::MissedTickBehavior;

use crate::game_monitor::LichessSettings;
use crate::heltour::HeltourSettings;
use crate::lichess;
use crate::zulip::ZulipSettings;
use checks::{Failure, Outcome, Token, ZulipBot};
pub use config::Config;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Health {
    pub postgres: Availability,
    pub heltour: Availability,
    pub lichess: Option<Availability<TokenOwner>>,
    pub zulip: Option<Availability>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Availability<T = ()> {
    Available(T),
    Unavailable(Unavailable),
}

impl<T> Availability<T> {
    #[must_use]
    pub const fn unavailable(&self) -> Option<&Unavailable> {
        match self {
            Self::Available(_) => None,
            Self::Unavailable(unavailable) => Some(unavailable),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unavailable {
    pub recovery: Recovery,
    pub error: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Recovery {
    Recoverable,
    NeedsAPerson,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TokenOwner(pub lichess::UserId);

#[derive(Debug)]
struct CheckEverything;

#[derive(Debug)]
pub struct HealthCheck {
    config: Config,
    health: watch::Sender<Option<Health>>,
    http: reqwest::Client,
    pool: PgPool,
    heltour: Token,
    lichess: Option<Token>,
    zulip: Option<ZulipBot>,
}

impl HealthCheck {
    pub fn new(
        config: Config,
        http: reqwest::Client,
        pool: PgPool,
        heltour: &HeltourSettings,
        lichess: &LichessSettings,
        zulip: Option<&ZulipSettings>,
    ) -> Result<Self, url::ParseError> {
        Ok(Self {
            config,
            health: watch::Sender::new(None),
            http,
            pool,
            heltour: Token::heltour(heltour)?,
            lichess: Token::lichess(lichess)?,
            zulip: zulip.map(ZulipBot::new).transpose()?,
        })
    }

    #[must_use]
    pub fn health(&self) -> watch::Receiver<Option<Health>> {
        self.health.subscribe()
    }

    pub async fn stopped(health: &ActorRef<Self>) -> anyhow::Error {
        match health.wait_for_shutdown_result().await {
            Ok(ActorStopReason::Panicked(error)) => error
                .with_str(|cause| anyhow::anyhow!("{cause}"))
                .unwrap_or_else(|| anyhow::anyhow!("health stopped: {error}")),
            _ => anyhow::anyhow!("health stopped unexpectedly"),
        }
    }

    fn check_every_interval(&self, health: WeakActorRef<Self>) {
        let mut ticks = tokio::time::interval(self.config.interval);
        ticks.set_missed_tick_behavior(MissedTickBehavior::Skip);
        tokio::spawn(async move {
            loop {
                ticks.tick().await;
                let Some(health) = health.upgrade() else {
                    return;
                };
                if health.tell(CheckEverything).await.is_err() {
                    return;
                }
            }
        });
    }

    async fn check_everything(&self) -> Result<(), String> {
        let (postgres, heltour, lichess, zulip) = tokio::join!(
            self.check("Postgres", || checks::read_our_tables(&self.pool)),
            self.check("heltour", || checks::test_heltour_token(
                &self.http,
                &self.heltour
            )),
            OptionFuture::from(self.lichess.as_ref().map(|token| {
                self.check("Lichess", || checks::lichess_token_owner(&self.http, token))
            })),
            OptionFuture::from(
                self.zulip
                    .as_ref()
                    .map(|bot| self.check("Zulip", || checks::find_zulip_bot(&self.http, bot)))
            ),
        );
        let health = Health {
            postgres: postgres?,
            heltour: heltour?,
            lichess: lichess.transpose()?,
            zulip: zulip.transpose()?,
        };
        self.publish(health);
        Ok(())
    }

    async fn check<T, F: Future<Output = Result<T, Failure>>>(
        &self,
        service: &str,
        mut check: impl FnMut() -> F,
    ) -> Result<Availability<T>, String> {
        let timeout = self.config.timeout;
        let backoff = ExponentialBuilder::new()
            .with_min_delay(self.config.initial_backoff)
            .without_max_delay()
            .with_max_times(self.config.retries);
        let checked = (|| within(timeout, check()))
            .retry(backoff)
            .sleep(tokio::time::sleep)
            .when(|failure| failure.outcome == Outcome::Retry)
            .notify(|failure, delay| {
                tracing::warn!(%service, error = %failure.error, retry_in = ?delay, "health check failed");
            })
            .await;
        availability(service, checked)
    }

    fn publish(&self, health: Health) {
        if self.health.borrow().as_ref() != Some(&health) {
            tracing::info!(?health, "health changed");
            self.health.send_replace(Some(health));
        }
    }
}

async fn within<T>(
    timeout: Duration,
    check: impl Future<Output = Result<T, Failure>>,
) -> Result<T, Failure> {
    tokio::time::timeout(timeout, check)
        .await
        .unwrap_or_else(|_| {
            Err(Failure::retry(anyhow::anyhow!(
                "no answer within {timeout:?}"
            )))
        })
}

fn availability<T>(service: &str, checked: Result<T, Failure>) -> Result<Availability<T>, String> {
    let unavailable = |recovery, error| Availability::Unavailable(Unavailable { recovery, error });
    match checked {
        Ok(value) => Ok(Availability::Available(value)),
        Err(Failure {
            outcome: Outcome::Retry,
            error,
        }) => Ok(unavailable(Recovery::Recoverable, error)),
        Err(Failure {
            outcome: Outcome::Report,
            error,
        }) => Ok(unavailable(Recovery::NeedsAPerson, error)),
        Err(Failure {
            outcome: Outcome::Exit,
            error,
        }) => {
            tracing::error!(%service, %error, "health check failed; exiting");
            Err(format!("{service} failed its health check: {error}"))
        }
    }
}

impl Actor for HealthCheck {
    type Args = Self;
    type Error = Infallible;

    fn on_start(
        health: Self,
        health_ref: ActorRef<Self>,
    ) -> impl Future<Output = Result<Self, Self::Error>> + Send {
        health.check_every_interval(health_ref.downgrade());
        std::future::ready(Ok(health))
    }
}

impl Message<CheckEverything> for HealthCheck {
    type Reply = Result<(), String>;

    async fn handle(
        &mut self,
        _: CheckEverything,
        _: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.check_everything().await
    }
}

#[cfg(test)]
mod tests;
