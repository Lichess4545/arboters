use std::collections::HashMap;
use std::convert::Infallible;
use std::ops::ControlFlow;

use backon::{BackoffBuilder, ExponentialBackoff, ExponentialBuilder};
use kameo::Actor;
use kameo::actor::{ActorRef, Recipient, ReplyRecipient, WeakRecipient};
use kameo::message::{Context, Message};
use tokio::task::JoinHandle;

use crate::chat::{
    ChatUserId, CommandText, Handling, IncomingMessage, Listen, MessageId, MessageText,
    Notification, NotificationTarget, Overhear, OverheardMessage, Revision, Role,
};

use super::api::{Api, ApiError, Bot, Delivery, Edit, Event, Flag, Place};
use super::messenger::ZulipMessenger;
use super::settings::{InboxSettings, ZulipSettings};

const UNRECOGNIZED: &str = "I don't know that command. Send `help` to see the commands I know.";

#[derive(Debug)]
pub struct Inbox {
    api: Api,
    settings: InboxSettings,
    messenger: ZulipMessenger,
    listeners: Vec<ReplyRecipient<IncomingMessage, Handling>>,
    overhearers: Vec<Recipient<OverheardMessage>>,
    conversations: HashMap<MessageId, Conversation>,
    reading: Option<JoinHandle<()>>,
}

#[derive(Debug)]
struct Conversation {
    reply_to: NotificationTarget,
    reply: Option<MessageId>,
}

#[derive(Debug)]
pub enum Received {
    Sent(IncomingMessage),
    Edited(EditedText),
}

#[derive(Debug)]
pub struct EditedText {
    id: MessageId,
    sender: ChatUserId,
    role: Role,
    text: CommandText,
}

#[derive(Debug)]
pub struct Overheard(OverheardMessage);

impl Inbox {
    #[must_use]
    pub fn new(http: reqwest::Client, settings: &ZulipSettings, messenger: ZulipMessenger) -> Self {
        Self {
            api: Api::new(http, settings),
            settings: settings.inbox.clone(),
            messenger,
            listeners: Vec::new(),
            overhearers: Vec::new(),
            conversations: HashMap::new(),
            reading: None,
        }
    }

    async fn answer(&self, message: &IncomingMessage) -> Notification {
        let mut answers = Vec::new();
        for listener in &self.listeners {
            match listener.ask(message.clone()).await {
                Ok(Handling::Answered(answer)) => answers.push(answer.markdown().to_owned()),
                Ok(Handling::Unrecognized) => {}
                Err(error) => tracing::warn!(error = ?error, "listener failed to take a message"),
            }
        }
        if answers.is_empty() {
            return Notification::new(UNRECOGNIZED.to_owned());
        }
        Notification::new(answers.join("\n\n"))
    }

    async fn reply(&mut self, message: IncomingMessage) {
        let answer = self.answer(&message).await;
        let earlier = self
            .conversations
            .get(&message.id)
            .and_then(|conversation| conversation.reply);
        let reply = match earlier {
            Some(reply) => match self.messenger.edit(&message.reply_to, reply, &answer).await {
                Ok(()) => Some(reply),
                Err(error) => {
                    tracing::warn!(error = %error, cause = %error.cause, "could not edit a reply");
                    Some(reply)
                }
            },
            None => match self.messenger.post(&message.reply_to, &answer).await {
                Ok(reply) => Some(reply),
                Err(error) => {
                    tracing::warn!(error = %error, cause = %error.cause, "could not reply to a message");
                    None
                }
            },
        };
        self.conversations.insert(
            message.id,
            Conversation {
                reply_to: message.reply_to,
                reply,
            },
        );
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
        let received = actor_ref.clone().recipient::<Received>().downgrade();
        let overheard = actor_ref.recipient::<Overheard>().downgrade();
        inbox.reading = Some(tokio::spawn(read(
            inbox.api.clone(),
            inbox.settings.clone(),
            received,
            overheard,
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

impl Message<Overhear> for Inbox {
    type Reply = ();

    async fn handle(&mut self, Overhear(overhearer): Overhear, _: &mut Context<Self, Self::Reply>) {
        self.overhearers.push(overhearer);
    }
}

impl Message<Overheard> for Inbox {
    type Reply = ();

    async fn handle(&mut self, Overheard(message): Overheard, _: &mut Context<Self, Self::Reply>) {
        for overhearer in &self.overhearers {
            if let Err(error) = overhearer.tell(message.clone()).await {
                tracing::warn!(error = ?error, "overhearer failed to take a message");
            }
        }
    }
}

impl Message<Received> for Inbox {
    type Reply = ();

    async fn handle(&mut self, received: Received, _: &mut Context<Self, Self::Reply>) {
        let message = match received {
            Received::Sent(message) => message,
            Received::Edited(edited) => {
                let Some(conversation) = self.conversations.get(&edited.id) else {
                    return;
                };
                IncomingMessage {
                    id: edited.id,
                    revision: Revision::Edited,
                    sender: edited.sender,
                    role: edited.role,
                    reply_to: conversation.reply_to.clone(),
                    text: edited.text,
                }
            }
        };
        self.reply(message).await;
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

async fn read(
    api: Api,
    settings: InboxSettings,
    inbox: WeakRecipient<Received>,
    overheard: WeakRecipient<Overheard>,
) {
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
                    for event in batch.events {
                        if let Event::Message(delivery) = &event
                            && let Some(message) = overhear(&bot, delivery)
                        {
                            if pass_on(message, &overheard).await.is_break() {
                                return;
                            }
                            continue;
                        }
                        if hand_over(&api, &bot, event, &inbox).await.is_break() {
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

enum Addressed {
    Sent {
        id: MessageId,
        sender: ChatUserId,
        reply_to: NotificationTarget,
        text: CommandText,
    },
    Edited {
        id: MessageId,
        sender: ChatUserId,
        text: CommandText,
    },
}

impl Addressed {
    const fn sender(&self) -> ChatUserId {
        match self {
            Self::Sent { sender, .. } | Self::Edited { sender, .. } => *sender,
        }
    }

    const fn text(&mut self) -> &mut CommandText {
        match self {
            Self::Sent { text, .. } | Self::Edited { text, .. } => text,
        }
    }

    fn with_role(self, role: Role) -> Received {
        match self {
            Self::Sent {
                id,
                sender,
                reply_to,
                text,
            } => Received::Sent(IncomingMessage {
                id,
                revision: Revision::Original,
                sender,
                role,
                reply_to,
                text,
            }),
            Self::Edited { id, sender, text } => Received::Edited(EditedText {
                id,
                sender,
                role,
                text,
            }),
        }
    }
}

async fn hand_over(
    api: &Api,
    bot: &Bot,
    event: Event,
    inbox: &WeakRecipient<Received>,
) -> ControlFlow<()> {
    let addressed = match event {
        Event::Message(delivery) => addressed(bot, delivery),
        Event::Edit(edit) => edited(bot, edit),
    };
    let Some(mut addressed) = addressed else {
        return ControlFlow::Continue(());
    };
    if let Some(text) = with_user_ids(api, addressed.text()).await {
        *addressed.text() = text;
    }
    let sender = addressed.sender();
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
    match inbox.tell(addressed.with_role(role)).await {
        Ok(()) => ControlFlow::Continue(()),
        Err(_) => ControlFlow::Break(()),
    }
}

async fn pass_on(
    message: OverheardMessage,
    overheard: &WeakRecipient<Overheard>,
) -> ControlFlow<()> {
    let Some(inbox) = overheard.upgrade() else {
        return ControlFlow::Break(());
    };
    match inbox.tell(Overheard(message)).await {
        Ok(()) => ControlFlow::Continue(()),
        Err(_) => ControlFlow::Break(()),
    }
}

fn overhear(bot: &Bot, Delivery { message, flags }: &Delivery) -> Option<OverheardMessage> {
    let Place::Stream {
        display_recipient,
        subject,
    } = &message.place
    else {
        return None;
    };
    (message.sender_id != bot.id && !flags.contains(&Flag::Mentioned)).then(|| OverheardMessage {
        sender: message.sender_id,
        channel: display_recipient.clone(),
        topic: subject.clone(),
        text: MessageText::new(message.content.clone()),
    })
}

fn addressed(bot: &Bot, Delivery { message, flags }: Delivery) -> Option<Addressed> {
    let sender = message.sender_id;
    if sender == bot.id {
        return None;
    }
    let sent = |reply_to, text| Addressed::Sent {
        id: message.id,
        sender,
        reply_to,
        text,
    };
    match message.place {
        Place::Private { display_recipient } => display_recipient
            .iter()
            .all(|recipient| recipient.id == sender || recipient.id == bot.id)
            .then(|| {
                sent(
                    NotificationTarget::DirectMessage { user_id: sender },
                    CommandText::new(&message.content),
                )
            }),
        Place::Stream {
            display_recipient,
            subject,
        } => flags.contains(&Flag::Mentioned).then(|| {
            sent(
                NotificationTarget::Channel {
                    channel: display_recipient,
                    topic: subject,
                },
                CommandText::new(without_mention(bot, &message.content)),
            )
        }),
    }
}

fn edited(bot: &Bot, edit: Edit) -> Option<Addressed> {
    let sender = edit.user_id.filter(|sender| *sender != bot.id)?;
    let content = edit.content.filter(|_| !edit.rendering_only)?;
    let text = match edit.stream_id {
        Some(_) if !edit.flags.contains(&Flag::Mentioned) => return None,
        Some(_) => without_mention(bot, &content),
        None => content.as_str(),
    };
    Some(Addressed::Edited {
        id: edit.message_id,
        sender,
        text: CommandText::new(text),
    })
}

async fn with_user_ids(api: &Api, text: &CommandText) -> Option<CommandText> {
    let mut unnamed = false;
    mention_ids(text, |_| {
        unnamed = true;
        None
    });
    if !unnamed {
        return None;
    }
    let members = match api.members().await {
        Ok(members) => members,
        Err(error) => {
            tracing::warn!(error = %error, "could not look up the users a message mentions");
            return None;
        }
    };
    Some(CommandText::new(mention_ids(text, |name| {
        let name = name.trim().to_lowercase();
        let mut named = members
            .iter()
            .filter(|member| member.is_active && member.full_name.to_lowercase() == name);
        match (named.next(), named.next()) {
            (Some(member), None) => Some(member.user_id),
            _ => None,
        }
    })))
}

fn mention_ids(text: &str, mut id_of: impl FnMut(&str) -> Option<ChatUserId>) -> String {
    let mut written = String::with_capacity(text.len());
    let mut rest = text;
    while let Some((before, after)) = rest.split_once('@') {
        written.push_str(before);
        written.push('@');
        let after = after.strip_prefix('_').map_or(after, |after| {
            written.push('_');
            after
        });
        rest = after;
        let Some((name, after)) = after
            .strip_prefix("**")
            .and_then(|inside| inside.split_once("**"))
        else {
            continue;
        };
        written.push_str("**");
        written.push_str(name);
        if !name.contains('|')
            && let Some(id) = id_of(name)
        {
            written.push('|');
            written.push_str(&id.0.to_string());
        }
        written.push_str("**");
        rest = after;
    }
    written.push_str(rest);
    written
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
