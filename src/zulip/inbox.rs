use std::convert::Infallible;
use std::ops::ControlFlow;

use backon::{BackoffBuilder, ExponentialBackoff, ExponentialBuilder};
use kameo::Actor;
use kameo::actor::{ActorRef, ReplyRecipient, WeakRecipient};
use kameo::message::{Context, Message};
use tokio::task::JoinHandle;

use crate::chat::{
    ChatUserId, CommandText, Handling, IncomingMessage, Listen, Messenger, Notification,
    NotificationTarget,
};

use super::api::{Api, ApiError, Bot, Delivery, Flag, Place};
use super::messenger::ZulipMessenger;
use super::settings::{InboxSettings, ZulipSettings};

const UNRECOGNIZED: &str = "I don't know that command. Send `help` to see the commands I know.";

#[derive(Debug)]
pub struct Inbox {
    api: Api,
    settings: InboxSettings,
    messenger: ZulipMessenger,
    listeners: Vec<ReplyRecipient<IncomingMessage, Handling>>,
    reading: Option<JoinHandle<()>>,
}

#[derive(Debug)]
pub struct Received(IncomingMessage);

impl Inbox {
    #[must_use]
    pub fn new(http: reqwest::Client, settings: &ZulipSettings, messenger: ZulipMessenger) -> Self {
        Self {
            api: Api::new(http, settings),
            settings: settings.inbox.clone(),
            messenger,
            listeners: Vec::new(),
            reading: None,
        }
    }

    async fn hand_out(&self, message: IncomingMessage) {
        let mut handled = false;
        for listener in &self.listeners {
            match listener.ask(message.clone()).await {
                Ok(Handling::Handled) => handled = true,
                Ok(Handling::Unrecognized) => {}
                Err(error) => tracing::warn!(error = ?error, "listener failed to take a message"),
            }
        }
        if handled {
            return;
        }
        let reply = Notification::new(UNRECOGNIZED.to_owned());
        if let Err(error) = self.messenger.send(&message.reply_to, &reply).await {
            tracing::warn!(
                error = %error,
                cause = %error.cause,
                "could not reply to an unrecognized message"
            );
        }
    }
}

impl Drop for Inbox {
    fn drop(&mut self) {
        if let Some(reading) = &self.reading {
            reading.abort();
        }
    }
}

impl Actor for Inbox {
    type Args = Self;
    type Error = Infallible;

    fn on_start(
        mut inbox: Self,
        actor_ref: ActorRef<Self>,
    ) -> impl Future<Output = Result<Self, Self::Error>> + Send {
        let received = actor_ref.recipient::<Received>().downgrade();
        inbox.reading = Some(tokio::spawn(read(
            inbox.api.clone(),
            inbox.settings.clone(),
            received,
        )));
        std::future::ready(Ok(inbox))
    }
}

impl Message<Listen> for Inbox {
    type Reply = ();

    async fn handle(&mut self, Listen(listener): Listen, _: &mut Context<Self, Self::Reply>) {
        self.listeners.push(listener);
    }
}

impl Message<Received> for Inbox {
    type Reply = ();

    async fn handle(&mut self, Received(message): Received, _: &mut Context<Self, Self::Reply>) {
        self.hand_out(message).await;
    }
}

struct Retry {
    delays: ExponentialBuilder,
    upcoming: ExponentialBackoff,
    max_backoff: std::time::Duration,
}

impl Retry {
    fn new(settings: &InboxSettings) -> Self {
        let delays = ExponentialBuilder::new()
            .with_min_delay(settings.initial_backoff)
            .with_max_delay(settings.max_backoff)
            .without_max_times();
        Self {
            delays,
            upcoming: delays.build(),
            max_backoff: settings.max_backoff,
        }
    }

    fn reset(&mut self) {
        self.upcoming = self.delays.build();
    }

    async fn wait(&mut self, error: &ApiError, doing: &'static str) {
        let delay = self.upcoming.next().unwrap_or(self.max_backoff);
        tracing::warn!(error = %error, cause = ?std::error::Error::source(error), retry_in = ?delay, doing, "zulip request failed");
        tokio::time::sleep(delay).await;
    }
}

async fn read(api: Api, settings: InboxSettings, inbox: WeakRecipient<Received>) {
    let mut retry = Retry::new(&settings);
    let bot = loop {
        match api.own_user().await {
            Ok(bot) => break bot,
            Err(error) => retry.wait(&error, "looking up the bot user").await,
        }
    };
    retry.reset();
    loop {
        let mut queue = match api.register().await {
            Ok(queue) => queue,
            Err(error) => {
                retry.wait(&error, "registering an event queue").await;
                continue;
            }
        };
        tracing::info!(queue_id = %queue.id, "zulip event queue registered");
        loop {
            match api.events(&queue).await {
                Ok(batch) => {
                    retry.reset();
                    queue.acknowledge(&batch);
                    for delivery in batch.messages {
                        if hand_over(&api, &bot, delivery, &inbox).await.is_break() {
                            return;
                        }
                    }
                }
                Err(ApiError::BadEventQueue) => {
                    tracing::info!(queue_id = %queue.id, "zulip event queue expired");
                    break;
                }
                Err(error) => retry.wait(&error, "reading events").await,
            }
        }
    }
}

async fn hand_over(
    api: &Api,
    bot: &Bot,
    delivery: Delivery,
    inbox: &WeakRecipient<Received>,
) -> ControlFlow<()> {
    let Some((sender, reply_to, text)) = addressed(bot, delivery) else {
        return ControlFlow::Continue(());
    };
    let role = match api.role_of(sender).await {
        Ok(role) => role,
        Err(error) => {
            tracing::warn!(error = %error, sender = sender.0, "could not look up the sender's role; message dropped");
            return ControlFlow::Continue(());
        }
    };
    let Some(inbox) = inbox.upgrade() else {
        return ControlFlow::Break(());
    };
    let message = IncomingMessage {
        sender,
        role,
        reply_to,
        text,
    };
    match inbox.tell(Received(message)).await {
        Ok(()) => ControlFlow::Continue(()),
        Err(_) => ControlFlow::Break(()),
    }
}

fn addressed(
    bot: &Bot,
    Delivery { message, flags }: Delivery,
) -> Option<(ChatUserId, NotificationTarget, CommandText)> {
    let sender = message.sender_id;
    if sender == bot.id {
        return None;
    }
    match message.place {
        Place::Private { display_recipient } => display_recipient
            .iter()
            .all(|recipient| recipient.id == sender || recipient.id == bot.id)
            .then(|| {
                (
                    sender,
                    NotificationTarget::DirectMessage { user_id: sender },
                    CommandText::new(message.content),
                )
            }),
        Place::Stream {
            display_recipient,
            subject,
        } => flags.contains(&Flag::Mentioned).then(|| {
            (
                sender,
                NotificationTarget::Channel {
                    channel: display_recipient,
                    topic: subject,
                },
                CommandText::new(without_mention(bot, &message.content)),
            )
        }),
    }
}

fn without_mention<'a>(bot: &Bot, content: &'a str) -> &'a str {
    let content = content.trim_start();
    let name = bot.full_name.as_str();
    let mention = format!("@**{name}**");
    let mention_with_id = format!("@**{name}|{}**", bot.id.0);
    content
        .strip_prefix(mention.as_str())
        .or_else(|| content.strip_prefix(mention_with_id.as_str()))
        .unwrap_or(content)
}

#[cfg(test)]
mod tests;
