pub mod config;
mod database;
mod http;
pub mod telemetry;

use kameo::actor::{ActorRef, Spawn};
use rand::SeedableRng;
use rand::rngs::StdRng;
use sqlx::PgPool;
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::{broadcast, watch};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::commands::{Commands, Listener};
use crate::game_monitor::{
    self, GameEvent, GameMonitor, PostgresFixturesStore, PostgresPublishedEventsStore, http_client,
};
use crate::health::{Health, HealthCheck};
use crate::heltour::{self, ActiveLeagues, Heltour};
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
    let (subscription_commands, subscriptions_listener) =
        start_subscription_commands(&config, &subscriptions, &active_leagues);
    let (response_commands, responses_listener) = start_response_commands(&config, &http, &pool);

    let (health_check, health) = start_health(&config, &http, &pool, heltour.clone(), &lichess_api);
    let status = Status::spawn((status, health.clone()));
    let conversations = Conversations::spawn(
        &http,
        &config.zulip,
        &messenger,
        vec![subscriptions_listener, responses_listener],
    );
    let responding = Responding::start(
        response_commands,
        &conversations,
        &config,
        &pool,
        &messenger,
    );
    let monitor = GameMonitor::new(
        lichess_api.clone(),
        config.lichess.clone(),
        PostgresFixturesStore::new(pool.clone()),
        PostgresPublishedEventsStore::new(pool.clone()),
        status.clone().recipient(),
    );
    let notifier = Notifier::new(subscriptions.clone(), messenger, lichess.clone());
    let subscribing = Subscribing::start(subscription_commands, notifier, monitor.events()).await;
    let mut games = Games::start(
        &config,
        heltour,
        lichess_api,
        (monitor, health.clone()),
        active_leagues,
    )
    .await;

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
        error = subscribing.stopped() => Err(error),
    };

    games.stop().await;
    conversations.stop().await;
    responding.stop().await;
    subscribing.stop().await;
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

fn start_subscription_commands(
    config: &Config,
    subscriptions: &PostgresSubscriptions,
    active_leagues: &watch::Sender<ActiveLeagues>,
) -> (
    ActorRef<SubscriptionCommands<PostgresSubscriptions>>,
    Listener,
) {
    SubscriptionCommands::new(
        subscriptions.clone(),
        config.lichess.base_url.clone(),
        active_leagues.subscribe(),
    )
    .listen()
}

fn start_response_commands(
    config: &Config,
    http: &reqwest::Client,
    pool: &PgPool,
) -> (
    ActorRef<ResponseCommands<PostgresResponses, ZulipMessageReader>>,
    Listener,
) {
    ResponseCommands::new(
        PostgresResponses::new(pool.clone()),
        ZulipMessageReader::new(http.clone(), &config.zulip),
        config.zulip.site.clone(),
        &config.responses,
    )
    .listen()
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
    monitor: ActorRef<GameMonitor<PostgresFixturesStore, PostgresPublishedEventsStore>>,
    reporter: Option<ActorRef<Reporter>>,
    sync: JoinHandle<()>,
    recheck: JoinHandle<()>,
}

impl Games {
    async fn start(
        config: &Config,
        heltour: Heltour,
        lichess: lichess::Api,
        (monitor, health): (
            GameMonitor<PostgresFixturesStore, PostgresPublishedEventsStore>,
            watch::Receiver<Option<Health>>,
        ),
        active_leagues: watch::Sender<ActiveLeagues>,
    ) -> Self {
        let reporter = if config.league_games.report {
            let reporter = Reporter::spawn((
                Reporter::new(heltour.clone(), config.lichess.base_url.clone()),
                monitor.events(),
            ));
            reporter.wait_for_startup().await;
            Some(reporter)
        } else {
            tracing::info!("reports to Heltour are off");
            None
        };
        let active_fixtures = monitor.active_fixtures();
        let monitor = GameMonitor::spawn((monitor, health));
        monitor.wait_for_startup().await;
        let sync = tokio::spawn(heltour::sync::keep_in_sync(
            heltour,
            monitor.clone().recipient(),
            active_leagues,
            config.heltour_sync.refresh_interval,
        ));
        let recheck = tokio::spawn(game_monitor::recheck::keep_rechecking(
            lichess,
            active_fixtures,
            monitor.clone().recipient(),
            config.game_recheck,
        ));
        Self {
            monitor,
            reporter,
            sync,
            recheck,
        }
    }

    async fn stopped(&mut self) -> anyhow::Error {
        tokio::select! {
            () = self.monitor.wait_for_shutdown() => anyhow::anyhow!("game monitor stopped unexpectedly"),
            error = reporter_stopped(self.reporter.as_ref()) => error,
            exit = &mut self.sync => match exit {
                Ok(()) => anyhow::anyhow!("heltour sync stopped unexpectedly"),
                Err(error) => error.into(),
            },
            exit = &mut self.recheck => match exit {
                Ok(()) => anyhow::anyhow!("finished games recheck stopped unexpectedly"),
                Err(error) => error.into(),
            },
        }
    }

    async fn stop(&self) {
        self.sync.abort();
        self.recheck.abort();
        stop(&self.monitor).await;
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
    commands: ActorRef<Commands>,
    inbox: ActorRef<Inbox>,
}

impl Conversations {
    fn spawn(
        http: &reqwest::Client,
        zulip: &zulip::Config,
        messenger: &ZulipMessenger,
        listeners: Vec<Listener>,
    ) -> Self {
        let commands = Commands::spawn(Commands::new(listeners));
        let inbox = Inbox::spawn(Inbox::new(
            http.clone(),
            zulip,
            messenger.clone(),
            commands.clone().reply_recipient(),
        ));
        Self { commands, inbox }
    }

    async fn stopped(&self) -> anyhow::Error {
        tokio::select! {
            () = self.commands.wait_for_shutdown() => anyhow::anyhow!("commands stopped unexpectedly"),
            () = self.inbox.wait_for_shutdown() => anyhow::anyhow!("zulip inbox stopped unexpectedly"),
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
        commands: ActorRef<ResponseCommands<PostgresResponses, ZulipMessageReader>>,
        conversations: &Conversations,
        config: &Config,
        pool: &PgPool,
        messenger: &ZulipMessenger,
    ) -> Self {
        let responder = Responder::spawn((
            Responder::new(
                PostgresResponses::new(pool.clone()),
                messenger.clone(),
                &config.responses,
                StdRng::from_os_rng(),
            ),
            conversations.inbox.clone().recipient(),
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
        stop(&self.responder).await;
        stop(&self.commands).await;
    }
}

struct Subscribing {
    commands: ActorRef<SubscriptionCommands<PostgresSubscriptions>>,
    notifier: ActorRef<Notifier<PostgresSubscriptions, ZulipMessenger>>,
}

impl Subscribing {
    async fn start(
        commands: ActorRef<SubscriptionCommands<PostgresSubscriptions>>,
        notifier: Notifier<PostgresSubscriptions, ZulipMessenger>,
        events: broadcast::Receiver<GameEvent>,
    ) -> Self {
        let notifier = Notifier::spawn((notifier, events));
        notifier.wait_for_startup().await;
        Self { commands, notifier }
    }

    async fn stopped(&self) -> anyhow::Error {
        tokio::select! {
            () = self.commands.wait_for_shutdown() => anyhow::anyhow!("subscription commands stopped unexpectedly"),
            () = self.notifier.wait_for_shutdown() => anyhow::anyhow!("notifier stopped unexpectedly"),
        }
    }

    async fn stop(&self) {
        stop(&self.notifier).await;
        stop(&self.commands).await;
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
