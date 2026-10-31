use std::convert::Infallible;
use std::fmt;
use std::time::{Duration, SystemTime};

use kameo::Actor;
use kameo::actor::ActorRef;
use kameo::message::{Context, Message};
use serde::Deserialize;
use tokio::time::Instant;
use uuid::Uuid;

use crate::chat::{
    ChannelName, Conversation, Messenger, OutgoingMessage, TopicName, TopicNameError,
};
use crate::game_monitor::StreamConnected;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatusSettings {
    pub channel: ChannelName,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct InstanceId(Uuid);

impl InstanceId {
    fn generate() -> Self {
        Self(Uuid::new_v4())
    }
}

impl fmt::Display for InstanceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// ```
/// use arboters::status::Signal;
///
/// assert_eq!(Signal::Interrupt.to_string(), "SIGINT");
/// assert_eq!(Signal::Terminate.to_string(), "SIGTERM");
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Signal {
    Interrupt,
    Terminate,
}

impl fmt::Display for Signal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Interrupt => "SIGINT",
            Self::Terminate => "SIGTERM",
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Shutdown {
    Requested(Signal),
    Failed(String),
}

#[derive(Debug)]
pub struct Status<M> {
    messenger: M,
    instance: InstanceId,
    conversation: Option<Conversation>,
    started: Instant,
}

impl<M: Messenger> Status<M> {
    pub fn new(messenger: M, settings: Option<StatusSettings>) -> Result<Self, TopicNameError> {
        let instance = InstanceId::generate();
        let conversation = settings
            .map(|settings| {
                Ok(Conversation::Topic {
                    channel: settings.channel,
                    topic: topic_for(instance, SystemTime::now())?,
                })
            })
            .transpose()?;
        Ok(Self {
            messenger,
            instance,
            conversation,
            started: Instant::now(),
        })
    }

    async fn post(&self, markdown: String) {
        let message = OutgoingMessage::new(markdown);
        let Some(conversation) = &self.conversation else {
            tracing::info!(
                instance = %self.instance,
                status = message.markdown(),
                "status not posted because no status channel is configured"
            );
            return;
        };
        if let Err(error) = self.messenger.send(conversation, &message).await {
            tracing::warn!(
                error = %error,
                cause = %error.cause,
                instance = %self.instance,
                ?conversation,
                "status delivery failed"
            );
        }
    }

    fn uptime(&self) -> humantime::FormattedDuration {
        humantime::format_duration(Duration::from_secs(self.started.elapsed().as_secs()))
    }
}

fn topic_for(instance: InstanceId, started: SystemTime) -> Result<TopicName, TopicNameError> {
    let timestamp = humantime::format_rfc3339_seconds(started).to_string();
    let date = timestamp
        .split_once('T')
        .map_or(timestamp.as_str(), |(date, _)| date);
    TopicName::try_new(format!("v{VERSION} {instance} {date}"))
}

impl<M: Messenger> Actor for Status<M> {
    type Args = Self;
    type Error = Infallible;

    async fn on_start(status: Self, _: ActorRef<Self>) -> Result<Self, Self::Error> {
        tracing::info!(instance = %status.instance, conversation = ?status.conversation, "arboters started");
        status
            .post(format!(
                ":rocket: arboters v{VERSION} started — [changelog]({REPOSITORY}/releases/tag/v{VERSION})"
            ))
            .await;
        Ok(status)
    }
}

impl<M: Messenger> Message<StreamConnected> for Status<M> {
    type Reply = ();

    async fn handle(
        &mut self,
        StreamConnected { players }: StreamConnected,
        _: &mut Context<Self, Self::Reply>,
    ) {
        self.post(format!(
            ":chess_pawn: Connected to the Lichess game stream, watching {players} players"
        ))
        .await;
    }
}

impl<M: Messenger> Message<Shutdown> for Status<M> {
    type Reply = ();

    async fn handle(&mut self, shutdown: Shutdown, _: &mut Context<Self, Self::Reply>) {
        let uptime = self.uptime();
        self.post(match shutdown {
            Shutdown::Requested(signal) => {
                format!(":octagonal_sign: Shutting down ({signal}) after {uptime}")
            }
            Shutdown::Failed(error) => {
                format!(":octagonal_sign: Shutting down after {uptime}: {error}")
            }
        })
        .await;
    }
}

#[cfg(test)]
mod tests;
