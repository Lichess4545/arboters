pub mod config;
mod database;
mod http;
pub mod telemetry;

use kameo::actor::{ActorRef, Spawn};
use sqlx::PgPool;
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use crate::game_monitor::watchlist::{self, Heltour};
use crate::game_monitor::{GameMonitor, LichessStream, TokenCheck, TokenHealth, http_client};
use crate::notifications::{LoggingMessenger, Messenger, Notifier};
use crate::status::{Shutdown, Signal, Status};
use crate::subscriptions::PostgresSubscriptions;
use crate::zulip::ZulipMessenger;
use config::Config;

pub async fn run(config: Config) -> anyhow::Result<()> {
    let pool = database::connect(&config.database).await?;
    if let Some(zulip) = &config.zulip {
        let messenger = ZulipMessenger::new(zulip)?;
        run_with(config, pool, messenger).await
    } else {
        tracing::warn!("zulip is not configured; notifications will only be logged");
        run_with(config, pool, LoggingMessenger).await
    }
}

async fn run_with<M: Messenger + Clone>(
    config: Config,
    pool: PgPool,
    messenger: M,
) -> anyhow::Result<()> {
    let listener = http::bind(&config.http).await?;
    let subscriptions = PostgresSubscriptions::new(pool.clone());
    let http = http_client()?;
    let stream = LichessStream::new(http.clone(), &config.lichess)?;
    let heltour = Heltour::new(http.clone(), &config.heltour)?;
    let (token_check, lichess_token) = match &config.lichess.token {
        Some(token) => {
            let (check, health) = TokenCheck::new(http, &config.lichess, token.clone())?;
            (Some(check), health)
        }
        None => (None, watch::channel(TokenHealth::Usable).1),
    };
    let status = Status::new(messenger.clone(), config.status.clone())?;

    let status = Status::spawn(status);
    let token_check = token_check.map(TokenCheck::spawn);
    let notifier = Notifier::spawn(Notifier::new(
        config.league.clone(),
        subscriptions.clone(),
        messenger,
    ));
    let monitor = GameMonitor::spawn(GameMonitor::new(
        stream,
        config.lichess.stream.clone(),
        notifier.clone().recipient(),
        status.clone().recipient(),
    ));
    let mut watchlist_sync = tokio::spawn(watchlist::keep_in_sync(
        heltour,
        config.league.clone(),
        monitor.clone().recipient(),
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
        () = stopped(token_check.as_ref()) => Err(anyhow::anyhow!("lichess token check stopped unexpectedly")),
        exit = &mut watchlist_sync => match exit {
            Ok(()) => Err(anyhow::anyhow!("watchlist sync stopped unexpectedly")),
            Err(error) => Err(error.into()),
        },
    };

    watchlist_sync.abort();
    stop(&monitor).await;
    stop(&notifier).await;
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
