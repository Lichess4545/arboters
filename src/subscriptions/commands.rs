mod parsing;

use kameo::Actor;
use kameo::actor::{ActorRef, Recipient};
use kameo::error::SendError;
use kameo::message::{Context, Message};

use crate::chat::{
    ChatUserId, Handling, IncomingMessage, Listen, Messenger, Notification, NotificationTarget,
};
use crate::game_monitor::GameEventKind;
use crate::heltour::{ActiveLeagues, league};

use super::model::{NewSubscription, Remover, SubscriptionId, SubscriptionSource};
use super::{Subscriptions, SubscriptionsError};
use parsing::{Command, IS_OVER, Listener, Mistake, STARTS, Tell, parse};

#[derive(Debug)]
pub struct SubscriptionCommands<R, M> {
    active: ActiveLeagues,
    subscriptions: R,
    messenger: M,
}

#[derive(Debug, thiserror::Error)]
#[error("subscription commands could not listen to the inbox")]
pub struct InboxUnavailable(#[from] SendError<Listen>);

impl<R: Subscriptions, M: Messenger> SubscriptionCommands<R, M> {
    #[must_use]
    pub fn new(subscriptions: R, messenger: M) -> Self {
        Self {
            active: ActiveLeagues::default(),
            subscriptions,
            messenger,
        }
    }

    async fn reply_to(
        &self,
        message: &IncomingMessage,
        command: Result<Command, Mistake>,
    ) -> String {
        let outcome = match command {
            Ok(Command::Help) => Ok(self.help()),
            Ok(Command::Tell(tell)) => self.tell(message, tell).await,
            Ok(Command::List) => self.list(message.sender).await,
            Ok(Command::Remove(id)) => self.remove(message, id).await,
            Err(mistake) => Ok(self.explain(mistake)),
        };
        outcome.unwrap_or_else(|error| {
            tracing::error!(error = %error, cause = ?std::error::Error::source(&error), "subscription command failed");
            "Something went wrong on my side. Please try again later.".to_owned()
        })
    }

    async fn tell(
        &self,
        message: &IncomingMessage,
        tell: Tell,
    ) -> Result<String, SubscriptionsError> {
        if !self.active.includes(&tell.league) {
            return Ok(format!(
                "`{}` is not an active league. {}",
                tell.league,
                self.active_leagues()
            ));
        }
        let target = match tell.listener {
            Listener::Me => NotificationTarget::DirectMessage {
                user_id: message.sender,
            },
            Listener::Here => match &message.reply_to {
                NotificationTarget::DirectMessage { .. } => {
                    return Ok("`tell here` only works in a channel topic. In a direct message, use `tell me`, or name a topic with `tell #**channel>topic**`.".to_owned());
                }
                channel @ NotificationTarget::Channel { .. } => channel.clone(),
            },
            Listener::Topic { channel, topic } => NotificationTarget::Channel { channel, topic },
        };
        if matches!(target, NotificationTarget::Channel { .. }) && !message.role.moderates() {
            return Ok(
                "Only moderators can subscribe a topic. Use `tell me` to subscribe yourself."
                    .to_owned(),
            );
        }
        let subscription = NewSubscription {
            requester: message.sender,
            league: tell.league,
            source: tell
                .player
                .map_or(SubscriptionSource::League, SubscriptionSource::Player),
            event: tell.event,
            target,
        };
        let description = describe(
            &subscription.league,
            &subscription.source,
            subscription.event,
            &subscription.target,
        );
        let id = self.subscriptions.subscribe(subscription).await?;
        Ok(format!("Subscription `{id}`: {description}."))
    }

    async fn list(&self, requester: ChatUserId) -> Result<String, SubscriptionsError> {
        let subscriptions = self.subscriptions.requested_by(requester).await?;
        if subscriptions.is_empty() {
            return Ok("You have no subscriptions.".to_owned());
        }
        let lines: Vec<String> = subscriptions
            .iter()
            .map(|subscription| {
                format!(
                    "- `{}`: {}",
                    subscription.id,
                    describe(
                        &subscription.league,
                        &subscription.source,
                        subscription.event,
                        &subscription.target
                    )
                )
            })
            .collect();
        Ok(format!("Your subscriptions:\n{}", lines.join("\n")))
    }

    async fn remove(
        &self,
        message: &IncomingMessage,
        id: SubscriptionId,
    ) -> Result<String, SubscriptionsError> {
        let remover = if message.role.moderates() {
            Remover::Moderator
        } else {
            Remover::Requester(message.sender)
        };
        if self.subscriptions.remove(id, remover).await? {
            return Ok(format!("Removed subscription `{id}`."));
        }
        Ok(match remover {
            Remover::Moderator => format!("There is no subscription `{id}`."),
            Remover::Requester(_) => {
                format!("You have no subscription `{id}`. Use `subscription list` to see yours.")
            }
        })
    }

    fn active_leagues(&self) -> String {
        let tags: Vec<String> = self
            .active
            .leagues
            .iter()
            .map(|league| format!("`{}`", league.tag))
            .collect();
        if tags.is_empty() {
            "No league is active right now.".to_owned()
        } else {
            format!("The active leagues are {}.", tags.join(", "))
        }
    }

    fn example(&self) -> String {
        self.active
            .leagues
            .first()
            .map(|league| {
                format!(
                    "\nFor example: `tell me when {STARTS} in {}`, or `tell here when {IS_OVER} in {} for <lichess-player>`.",
                    league.tag, league.tag
                )
            })
            .unwrap_or_default()
    }

    fn help(&self) -> String {
        format!(
            "I know these commands:\n\
             - `tell me when <event> in <league>` sends you a direct message about every game in the league.\n\
             - `tell here when <event> in <league>` posts about every game in the topic where you send it.\n\
             - `tell #**channel>topic** when <event> in <league>` posts about every game in that topic. To write the link, type `#` and pick the topic.\n\
             - Add `for <lichess-player>` to the end of a `tell` command to hear about one player's games only.\n\
             - `subscription list` lists your subscriptions and their ids.\n\
             - `subscription remove <id>` removes one of your subscriptions. Moderators can remove anyone's.\n\
             \n\
             Only moderators can subscribe a topic. `<event>` is `{STARTS}` or `{IS_OVER}`. {}{}",
            self.active_leagues(),
            self.example()
        )
    }

    fn explain(&self, mistake: Mistake) -> String {
        match mistake {
            Mistake::Tell => "Use `tell me when <event> in <league> for <lichess-player>`, or leave out `for <lichess-player>` to hear about every game. Send `help` for more.".to_owned(),
            Mistake::Listener => "You can tell `me`, `here`, or a topic, such as `#**channel>topic**`.".to_owned(),
            Mistake::TopicLink => "A topic link looks like `#**channel>topic**`. To write one, type `#` and pick the topic.".to_owned(),
            Mistake::Event => format!("`<event>` is `{STARTS}` or `{IS_OVER}`."),
            Mistake::League => format!("`<league>` is a league tag. {}", self.active_leagues()),
            Mistake::Player => "That is not a Lichess username.".to_owned(),
            Mistake::Subscription => "Use `tell me when <event> in <league>` to add a subscription, `subscription list` to see yours, or `subscription remove <id>` to remove one.".to_owned(),
            Mistake::Id => {
                "A subscription id is a number. Use `subscription list` to see yours.".to_owned()
            }
        }
    }
}

fn describe(
    league: &league::Tag,
    source: &SubscriptionSource,
    event: GameEventKind,
    target: &NotificationTarget,
) -> String {
    let listener = match target {
        NotificationTarget::DirectMessage { .. } => "you".to_owned(),
        NotificationTarget::Channel { channel, topic } => {
            format!("#**{}>{}**", channel.as_str(), topic.as_str())
        }
    };
    let event = match event {
        GameEventKind::GameStarted => STARTS,
        GameEventKind::GameFinished => IS_OVER,
    };
    match source {
        SubscriptionSource::Player(player) => {
            format!("tell {listener} when {event} in {league} for {player}")
        }
        SubscriptionSource::Team(team) => {
            format!(
                "tell {listener} when {event} in {league} for team {}",
                team.as_str()
            )
        }
        SubscriptionSource::League => format!("tell {listener} when {event} in {league}"),
    }
}

impl<R: Subscriptions, M: Messenger> Actor for SubscriptionCommands<R, M> {
    type Args = (Self, Recipient<Listen>);
    type Error = InboxUnavailable;

    async fn on_start(
        (commands, inbox): Self::Args,
        actor_ref: ActorRef<Self>,
    ) -> Result<Self, Self::Error> {
        inbox
            .tell(Listen(actor_ref.reply_recipient::<IncomingMessage>()))
            .await?;
        Ok(commands)
    }
}

impl<R: Subscriptions, M: Messenger> Message<ActiveLeagues> for SubscriptionCommands<R, M> {
    type Reply = ();

    async fn handle(&mut self, active: ActiveLeagues, _: &mut Context<Self, Self::Reply>) {
        self.active = active;
    }
}

impl<R: Subscriptions, M: Messenger> Message<IncomingMessage> for SubscriptionCommands<R, M> {
    type Reply = Handling;

    async fn handle(
        &mut self,
        message: IncomingMessage,
        _: &mut Context<Self, Self::Reply>,
    ) -> Handling {
        let Some(command) = parse(&message.text) else {
            return Handling::Unrecognized;
        };
        let reply = Notification::new(self.reply_to(&message, command).await);
        if let Err(error) = self.messenger.send(&message.reply_to, &reply).await {
            tracing::warn!(
                error = %error,
                cause = %error.cause,
                "could not reply to a subscription command"
            );
        }
        Handling::Handled
    }
}

#[cfg(test)]
mod tests;
