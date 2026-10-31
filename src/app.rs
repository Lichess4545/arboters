pub mod config;
mod database;
mod http;
pub mod telemetry;

use kameo::actor::{ActorRef, Recipient, Spawn};
use rand::SeedableRng;
use rand::rngs::StdRng;
use sqlx::PgPool;
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use crate::chat::{LoggingMessenger, Messenger};
use crate::game_monitor::watchlist;
use crate::game_monitor::{GameMonitor, LichessSettings, LichessStream, http_client};
use crate::heltour::{ActiveLeagues, Heltour};
use crate::league_games::Reporter;
use crate::lichess_token_check::{TokenCheck, TokenState};
use crate::responses::{PostgresResponses, Responder, ResponseCommands};
use crate::status::{Shutdown, Signal, Status};
use crate::subscriptions::{Notifier, PostgresSubscriptions, SubscriptionCommands};
use crate::zulip::{Inbox, ZulipMessageReader, ZulipMessenger};
use config::Config;

pub async fn run(config: Config) -> anyhow::Result<()> {
    let pool = database::connect(&config.database).await?;
    let http = http_client()?;
    if let Some(zulip) = &config.zulip {
        let messenger = ZulipMessenger::new(zulip)?;
        let inbox = Inbox::new(http.clone(), zulip, messenger.clone());
        run_with(config, pool, http, messenger, Some(inbox)).await
    } else {
        tracing::warn!(
            "zulip is not configured; notifications will only be logged and no commands are read"
        );
        run_with(config, pool, http, LoggingMessenger, None).await
    }
}

async fn run_with<M: Messenger + Clone>(
    config: Config,
    pool: PgPool,
    http: reqwest::Client,
    messenger: M,
    inbox: Option<Inbox>,
) -> anyhow::Result<()> {
    let listener = http::bind(&config.http).await?;
    let subscriptions = PostgresSubscriptions::new(pool.clone());
    let stream = LichessStream::new(http.clone(), &config.lichess)?;
    let heltour = Heltour::new(http.clone(), &config.heltour)?;
    let (token_check, lichess_token) = lichess_token_check(http.clone(), &config.lichess)?;
    let status = Status::new(messenger.clone(), config.status.clone())?;
    let lichess = &config.lichess.base_url;
    let commands = SubscriptionCommands::new(subscriptions.clone(), lichess.clone());

    let status = Status::spawn(status);
    let token_check = token_check.map(TokenCheck::spawn);
    let conversations = match inbox {
        Some(inbox) => Some(Conversations::spawn(inbox, commands).await),
        None => None,
    };
    let responding = Responding::start(conversations.as_ref(), &config, &http, &pool, &messenger);
    let monitor = GameMonitor::spawn(GameMonitor::new(
        stream,
        config.lichess.stream.clone(),
        status.clone().recipient(),
        lichess_token.clone(),
    ));
    let notifier = Notifier::spawn((
        Notifier::new(subscriptions.clone(), messenger, lichess.clone()),
        monitor.clone().recipient(),
    ));
    let reporter = Reporter::spawn((
        Reporter::new(heltour.clone(), lichess.clone()),
        monitor.clone().recipient(),
    ));
    notifier.wait_for_startup().await;
    reporter.wait_for_startup().await;
    let mut followers = vec![notifier.clone().recipient(), reporter.clone().recipient()];
    followers.extend(conversations.iter().map(Conversations::follower));
    let mut watchlist_sync = tokio::spawn(watchlist::keep_in_sync(
        heltour,
        monitor.clone().recipient(),
        followers,
        config.watchlist.refresh_interval,
    ));

    let http_stopped = CancellationToken::new();
    let mut http_server = tokio::spawn(http::serve(
        listener,
        http::router(pool, config.http.readiness_timeout, lichess_token),
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
        () = monitor.wait_for_shutdown() => Err(anyhow::anyhow!("game monitor stopped unexpectedly")),
        () = notifier.wait_for_shutdown() => Err(anyhow::anyhow!("notifier stopped unexpectedly")),
        () = reporter.wait_for_shutdown() => Err(anyhow::anyhow!("heltour reporter stopped unexpectedly")),
        () = stopped(token_check.as_ref()) => Err(anyhow::anyhow!("lichess token check stopped unexpectedly")),
        error = Conversations::stopped(conversations.as_ref()) => Err(error),
        error = Responding::stopped(responding.as_ref()) => Err(error),
        exit = &mut watchlist_sync => match exit {
            Ok(()) => Err(anyhow::anyhow!("watchlist sync stopped unexpectedly")),
            Err(error) => Err(error.into()),
        },
    };

    watchlist_sync.abort();
    if let Some(conversations) = &conversations {
        conversations.stop().await;
    }
    Responding::stop(responding.as_ref()).await;
    stop(&monitor).await;
    stop(&notifier).await;
    stop(&reporter).await;
    if let Some(token_check) = &token_check {
        stop(token_check).await;
    }
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

    fn follower(&self) -> Recipient<ActiveLeagues> {
        self.commands.clone().recipient()
    }

    async fn stopped(conversations: Option<&Self>) -> anyhow::Error {
        let Some(conversations) = conversations else {
            return std::future::pending().await;
        };
        tokio::select! {
            () = conversations.inbox.wait_for_shutdown() => anyhow::anyhow!("zulip inbox stopped unexpectedly"),
            () = conversations.commands.wait_for_shutdown() => anyhow::anyhow!("subscription commands stopped unexpectedly"),
        }
    }

    async fn stop(&self) {
        stop(&self.inbox).await;
        stop(&self.commands).await;
    }
}

struct Responding<M: Messenger> {
    commands: ActorRef<ResponseCommands<PostgresResponses, ZulipMessageReader>>,
    responder: ActorRef<Responder<PostgresResponses, M>>,
}

impl<M: Messenger + Clone> Responding<M> {
    fn start(
        conversations: Option<&Conversations>,
        config: &Config,
        http: &reqwest::Client,
        pool: &PgPool,
        messenger: &M,
    ) -> Option<Self> {
        let inbox = &conversations?.inbox;
        let zulip = config.zulip.as_ref()?;
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
        Some(Self {
            commands,
            responder,
        })
    }

    async fn stopped(responding: Option<&Self>) -> anyhow::Error {
        let Some(responding) = responding else {
            return std::future::pending().await;
        };
        tokio::select! {
            () = responding.commands.wait_for_shutdown() => anyhow::anyhow!("response commands stopped unexpectedly"),
            () = responding.responder.wait_for_shutdown() => anyhow::anyhow!("responder stopped unexpectedly"),
        }
    }

    async fn stop(responding: Option<&Self>) {
        if let Some(responding) = responding {
            stop(&responding.commands).await;
            stop(&responding.responder).await;
        }
    }
}

fn lichess_token_check(
    http: reqwest::Client,
    lichess: &LichessSettings,
) -> Result<(Option<TokenCheck>, watch::Receiver<TokenState>), url::ParseError> {
    let Some(token) = &lichess.token else {
        return Ok((None, watch::channel(TokenState::Untested).1));
    };
    let (check, state) = TokenCheck::new(
        http,
        &lichess.base_url,
        token.clone(),
        lichess.token_check_interval,
    )?;
    Ok((Some(check), state))
}

async fn stop<A: kameo::Actor>(actor: &ActorRef<A>) {
    if actor.stop_gracefully().await.is_ok() {
        actor.wait_for_shutdown().await;
    }
}

async fn stopped<A: kameo::Actor>(actor: Option<&ActorRef<A>>) {
    match actor {
        Some(actor) => actor.wait_for_shutdown().await,
        None => std::future::pending().await,
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
