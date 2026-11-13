use std::convert::Infallible;
use std::fmt;

use futures::future::{BoxFuture, join_all};
use kameo::actor::{ActorRef, ReplyRecipient};
use kameo::message::{Context, Message};
use kameo::{Actor, Reply};

use crate::chat::{IncomingMessage, OutgoingMessage};

pub(crate) const UNRECOGNIZED: &str =
    "I don't know that command. Send `help` to see the commands I know.";

trait Hear: Send + Sync {
    fn hear(&self, message: &IncomingMessage) -> Option<BoxFuture<'static, Handling>>;
}

struct Domain<T: Send + 'static, P> {
    parse: P,
    answerer: ReplyRecipient<T, Handling>,
}

impl<T, P> Hear for Domain<T, P>
where
    T: fmt::Debug + Send + 'static,
    P: Fn(&IncomingMessage) -> Option<T> + Send + Sync,
{
    fn hear(&self, message: &IncomingMessage) -> Option<BoxFuture<'static, Handling>> {
        let found = (self.parse)(message)?;
        let answerer = self.answerer.clone();
        Some(Box::pin(async move {
            answerer.ask(found).await.unwrap_or_else(|error| {
                tracing::warn!(error = ?error, "listener failed to take a command");
                Handling::Declined
            })
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Reply)]
pub enum Handling {
    Answered(OutgoingMessage),
    Declined,
}

pub struct Listener(Box<dyn Hear>);

impl Listener {
    #[must_use]
    pub fn new<T, P>(parse: P, answerer: ReplyRecipient<T, Handling>) -> Self
    where
        T: fmt::Debug + Send + 'static,
        P: Fn(&IncomingMessage) -> Option<T> + Send + Sync + 'static,
    {
        Self(Box::new(Domain { parse, answerer }))
    }
}

impl fmt::Debug for Listener {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("Listener").finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub struct Commands {
    listeners: Vec<Listener>,
}

impl Commands {
    #[must_use]
    pub const fn new(listeners: Vec<Listener>) -> Self {
        Self { listeners }
    }
}

impl Actor for Commands {
    type Args = Self;
    type Error = Infallible;

    fn on_start(
        commands: Self,
        _: ActorRef<Self>,
    ) -> impl Future<Output = Result<Self, Self::Error>> + Send {
        std::future::ready(Ok(commands))
    }
}

impl Message<IncomingMessage> for Commands {
    type Reply = OutgoingMessage;

    async fn handle(
        &mut self,
        message: IncomingMessage,
        _: &mut Context<Self, Self::Reply>,
    ) -> OutgoingMessage {
        let matched = self
            .listeners
            .iter()
            .filter_map(|Listener(listener)| listener.hear(&message));
        let answers: Vec<String> = join_all(matched)
            .await
            .into_iter()
            .filter_map(|handling| match handling {
                Handling::Answered(answer) => Some(answer.markdown().to_owned()),
                Handling::Declined => None,
            })
            .collect();
        if answers.is_empty() {
            return OutgoingMessage::new(UNRECOGNIZED.to_owned());
        }
        OutgoingMessage::new(answers.join("\n\n"))
    }
}

#[cfg(test)]
mod tests;
