pub mod config;
mod database;
mod http;
pub mod telemetry;

use kameo::actor::{ActorRef, Spawn};
use rand::SeedableRng;
use rand::rngs::StdRng;
use sqlx::PgPool;
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::game_monitor::watchlist;
use crate::game_monitor::{GameMonitor, http_client};
use crate::health::{Health, HealthCheck};
use crate::heltour::{ActiveLeagues, Heltour};
use crate::league_games::Reporter;
use crate::lichess;
use crate::responses::{PostgresResponses, Responder, ResponseCommands};
use crate::status::{Shutdown, Signal, Status};
use crate::subscriptions::{Notifier, PostgresSubscriptions, SubscriptionCommands};
use crate::zulip::{self, Inbox, ZulipMessageReader, ZulipMessenger};
use config::Config;

pub async fn run(config: Config) -> anyhow::Result<()> {
    let pool = database::connect(&config.database).await?;
    let http = http_client()?;
    let messenger = ZulipMessenger::new(http.clone(), &config.zulip);
    let inbox = Inbox::new(http.clone(), &config.zulip, messenger.clone());
    let listener = http::bind(&config.http).await?;
    let subscriptions = PostgresSubscriptions::new(pool.clone());
    let lichess_api = lichess::Api::new(
        http.clone(),
        &config.lichess.base_url,
        config.lichess.token.clone(),
    );
    let heltour = Heltour::new(http.clone(), &config.heltour);
    let status = Status::new(messenger.clone(), config.status.clone())?;
    let lichess = &config.lichess.base_url;
    let active_leagues = watch::Sender::new(ActiveLeagues::default());
    let commands = SubscriptionCommands::new(
        subscriptions.clone(),
        lichess.clone(),
        active_leagues.subscribe(),
    );

    let (health_check, health) = start_health(&config, &http, &pool, heltour.clone(), &lichess_api);
    let status = Status::spawn((status, health.clone()));
    let conversations = Conversations::spawn(inbox, commands).await;
    let responding = Responding::start(&conversations, &config, &http, &pool, &messenger);
    let monitor = GameMonitor::new(
        lichess_api,
        config.lichess.clone(),
        status.clone().recipient(),
        health.clone(),
    );
    let notifier = Notifier::new(
        subscriptions.clone(),
        messenger,
        lichess.clone(),
        active_leagues.subscribe(),
    );
    let mut games = Games::start(&config, heltour, monitor, notifier, active_leagues).await;

    let http_stopped = CancellationToken::new();
    let mut http_server = tokio::spawn(http::serve(
        listener,
        http::router(health),
        http_stopped.clone().cancelled_owned(),
    ));
    let mut http_server_running = true;

    let down: anyhow::Result<Signal> = tokio::select! {
        signal = shutdown_requested() => Ok(signal),
        exit = &mut http_server => {
            http_server_running = false;
            match exit {
                Ok(Ok(())) => Err(anyhow::anyhow!("http server stopped unexpectedly")),
                Ok(Err(error)) => Err(error.into()),
                Err(error) => Err(error.into()),
            }
        }
        () = status.wait_for_shutdown() => Err(anyhow::anyhow!("status stopped unexpectedly")),
        error = HealthCheck::stopped(&health_check) => Err(error),
        error = games.stopped() => Err(error),
        error = conversations.stopped() => Err(error),
        error = responding.stopped() => Err(error),
    };

    games.stop().await;
    conversations.stop().await;
    responding.stop().await;
    stop(&health_check).await;
    let shutdown = match &down {
        Ok(signal) => Shutdown::Requested(*signal),
        Err(error) => Shutdown::Failed(format!("{error:#}")),
    };
    if status.ask(shutdown).await.is_err() {
        tracing::warn!("status is not running; shutdown not posted");
    }
    stop(&status).await;
    http_stopped.cancel();
    let http_exit = if http_server_running {
        http_server
            .await
            .map_err(anyhow::Error::from)
            .and_then(|served| Ok(served?))
    } else {
        Ok(())
    };

    exit_with(down, http_exit)
}

fn start_health(
    config: &Config,
    http: &reqwest::Client,
    pool: &PgPool,
    heltour: Heltour,
    lichess: &lichess::Api,
) -> (ActorRef<HealthCheck>, watch::Receiver<Option<Health>>) {
    let health_check = HealthCheck::new(
        config.health,
        pool.clone(),
        heltour,
        config.lichess.token.is_some().then(|| lichess.clone()),
        zulip::Api::new(http.clone(), &config.zulip),
    );
    let health = health_check.health();
    (HealthCheck::spawn(health_check), health)
}

struct Games {
    monitor: ActorRef<GameMonitor>,
    notifier: ActorRef<Notifier<PostgresSubscriptions, ZulipMessenger>>,
    reporter: Option<ActorRef<Reporter>>,
    sync: JoinHandle<()>,
}

impl Games {
    async fn start(
        config: &Config,
        heltour: Heltour,
        monitor: GameMonitor,
        notifier: Notifier<PostgresSubscriptions, ZulipMessenger>,
        active_leagues: watch::Sender<ActiveLeagues>,
    ) -> Self {
        let notifier = Notifier::spawn((notifier, monitor.events()));
        notifier.wait_for_startup().await;
        let reporter = if config.league_games.report {
            let reporter = Reporter::spawn((
                Reporter::new(
                    heltour.clone(),
                    config.lichess.base_url.clone(),
                    active_leagues.subscribe(),
                ),
                monitor.events(),
            ));
            reporter.wait_for_startup().await;
            Some(reporter)
        } else {
            tracing::info!("reports to Heltour are off");
            None
        };
        let monitor = GameMonitor::spawn(monitor);
        let sync = tokio::spawn(watchlist::keep_in_sync(
            heltour,
            monitor.clone().recipient(),
            active_leagues,
            config.watchlist.refresh_interval,
        ));
        Self {
            monitor,
            notifier,
            reporter,
            sync,
        }
    }

    async fn stopped(&mut self) -> anyhow::Error {
        tokio::select! {
            () = self.monitor.wait_for_shutdown() => anyhow::anyhow!("game monitor stopped unexpectedly"),
            () = self.notifier.wait_for_shutdown() => anyhow::anyhow!("notifier stopped unexpectedly"),
            error = reporter_stopped(self.reporter.as_ref()) => error,
            exit = &mut self.sync => match exit {
                Ok(()) => anyhow::anyhow!("watchlist sync stopped unexpectedly"),
                Err(error) => error.into(),
            },
        }
    }

    async fn stop(&self) {
        self.sync.abort();
        stop(&self.monitor).await;
        stop(&self.notifier).await;
        if let Some(reporter) = &self.reporter {
            stop(reporter).await;
        }
    }
}

async fn reporter_stopped(reporter: Option<&ActorRef<Reporter>>) -> anyhow::Error {
    let Some(reporter) = reporter else {
        return std::future::pending().await;
    };
    reporter.wait_for_shutdown().await;
    anyhow::anyhow!("Heltour reporter stopped unexpectedly")
}

fn exit_with(down: anyhow::Result<Signal>, http_exit: anyhow::Result<()>) -> anyhow::Result<()> {
    match down {
        Ok(_) => {
            http_exit?;
            tracing::info!("shutdown complete");
            Ok(())
        }
        Err(error) => {
            if let Err(http_error) = http_exit {
                tracing::warn!(error = %http_error, "http server failed while shutting down");
            }
            Err(error)
        }
    }
}

struct Conversations {
    inbox: ActorRef<Inbox>,
    commands: ActorRef<SubscriptionCommands<PostgresSubscriptions>>,
}

impl Conversations {
    async fn spawn(inbox: Inbox, commands: SubscriptionCommands<PostgresSubscriptions>) -> Self {
        let inbox = Inbox::spawn(inbox);
        let commands = SubscriptionCommands::spawn((commands, inbox.clone().recipient()));
        commands.wait_for_startup().await;
        Self { inbox, commands }
    }

    async fn stopped(&self) -> anyhow::Error {
        tokio::select! {
            () = self.inbox.wait_for_shutdown() => anyhow::anyhow!("zulip inbox stopped unexpectedly"),
            () = self.commands.wait_for_shutdown() => anyhow::anyhow!("subscription commands stopped unexpectedly"),
        }
    }

    async fn stop(&self) {
        stop(&self.inbox).await;
        stop(&self.commands).await;
    }
}

struct Responding {
    commands: ActorRef<ResponseCommands<PostgresResponses, ZulipMessageReader>>,
    responder: ActorRef<Responder<PostgresResponses, ZulipMessenger>>,
}

impl Responding {
    fn start(
        conversations: &Conversations,
        config: &Config,
        http: &reqwest::Client,
        pool: &PgPool,
        messenger: &ZulipMessenger,
    ) -> Self {
        let inbox = &conversations.inbox;
        let zulip = &config.zulip;
        let responses = PostgresResponses::new(pool.clone());
        let commands = ResponseCommands::spawn((
            ResponseCommands::new(
                responses.clone(),
                ZulipMessageReader::new(http.clone(), zulip),
                zulip.site.clone(),
                &config.responses,
            ),
            inbox.clone().recipient(),
        ));
        let responder = Responder::spawn((
            Responder::new(
                responses,
                messenger.clone(),
                &config.responses,
                StdRng::from_os_rng(),
            ),
            inbox.clone().recipient(),
        ));
        Self {
            commands,
            responder,
        }
    }

    async fn stopped(&self) -> anyhow::Error {
        tokio::select! {
            () = self.commands.wait_for_shutdown() => anyhow::anyhow!("response commands stopped unexpectedly"),
            () = self.responder.wait_for_shutdown() => anyhow::anyhow!("responder stopped unexpectedly"),
        }
    }

    async fn stop(&self) {
        stop(&self.commands).await;
        stop(&self.responder).await;
    }
}

async fn stop<A: kameo::Actor>(actor: &ActorRef<A>) {
    if actor.stop_gracefully().await.is_ok() {
        actor.wait_for_shutdown().await;
    }
}

async fn shutdown_requested() -> Signal {
    let terminate = async {
        match signal(SignalKind::terminate()) {
            Ok(mut terminations) => {
                terminations.recv().await;
            }
            Err(error) => {
                tracing::warn!(error = %error, "cannot listen for SIGTERM");
                std::future::pending::<()>().await;
            }
        }
    };
    let received = tokio::select! {
        _ = tokio::signal::ctrl_c() => Signal::Interrupt,
        () = terminate => Signal::Terminate,
    };
    tracing::info!(signal = %received, "shutdown requested");
    received
}
