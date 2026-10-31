mod league_lookup;
mod parsing;
mod spelling;

use std::collections::HashMap;

use kameo::Actor;
use kameo::actor::{ActorRef, Recipient};
use kameo::error::SendError;
use kameo::message::{Context, Message};
use url::Url;

use crate::chat::{
    ChatUserId, Handling, IncomingMessage, Listen, MessageId, Notification, NotificationTarget,
    Revision,
};
use crate::game_monitor::GameEventKind;
use crate::heltour::{ActiveLeagues, League, league};

use super::model::{NewSubscription, Remover, SubscriptionId, SubscriptionSource};
use super::{Subscriptions, SubscriptionsError};
use league_lookup::Found;
use parsing::{Command, Draft, IS_OVER, Listener, Mistake, Problem, STARTS, Tell, parse};

#[derive(Debug)]
pub struct SubscriptionCommands<R> {
    active: ActiveLeagues,
    subscriptions: R,
    subscribed_by: HashMap<MessageId, (SubscriptionId, NewSubscription)>,
    lichess: Url,
}

#[derive(Debug, thiserror::Error)]
#[error("subscription commands could not listen to the inbox")]
pub struct InboxUnavailable(#[from] SendError<Listen>);

impl<R: Subscriptions> SubscriptionCommands<R> {
    #[must_use]
    pub fn new(subscriptions: R, lichess: Url) -> Self {
        Self {
            active: ActiveLeagues::default(),
            subscriptions,
            subscribed_by: HashMap::new(),
            lichess,
        }
    }

    fn earlier_subscription(&self, message: &IncomingMessage) -> Option<SubscriptionId> {
        match message.revision {
            Revision::Original => None,
            Revision::Edited => self.subscribed_by.get(&message.id).map(|(id, _)| *id),
        }
    }

    async fn reply_to(
        &mut self,
        message: &IncomingMessage,
        command: Result<Command, Mistake>,
    ) -> String {
        let outcome = match command {
            Ok(Command::Help) => Ok(self.help(message)),
            Ok(Command::Tell(tell)) => match self.request(message, tell) {
                Ok(subscription) => self.subscribe(message, subscription).await,
                Err(refusal) => Ok(self.unchanged(message, refusal)),
            },
            Ok(Command::List) => self.list(message).await,
            Ok(Command::Remove(id)) => self.remove(message, id).await,
            Err(mistake) => Ok(self.unchanged(message, self.explain(message, mistake))),
        };
        outcome.unwrap_or_else(|error| {
            tracing::error!(error = %error, cause = ?std::error::Error::source(&error), "subscription command failed");
            "Something went wrong on my side. Please try again later.".to_owned()
        })
    }

    fn unchanged(&self, message: &IncomingMessage, reply: String) -> String {
        match self.earlier_subscription(message) {
            Some(id) => format!("{reply}\nSubscription `{id}` is unchanged."),
            None => reply,
        }
    }

    fn request(&self, message: &IncomingMessage, tell: Tell) -> Result<NewSubscription, String> {
        let league = match league_lookup::find(&self.active, &tell.league) {
            Found::One(league) => league.tag.clone(),
            found => return Err(self.unknown_league(message, &Draft::from(tell), &found)),
        };
        let target = match tell.listener {
            Listener::Me => NotificationTarget::DirectMessage {
                user_id: message.sender,
            },
            Listener::Here => match &message.reply_to {
                NotificationTarget::DirectMessage { .. } => {
                    return Err("`tell here` only works in a channel topic. In a direct message, use `tell me`, or name a topic with `tell #**channel>topic**`.".to_owned());
                }
                channel @ NotificationTarget::Channel { .. } => channel.clone(),
            },
            Listener::Topic { channel, topic } => NotificationTarget::Channel { channel, topic },
            Listener::User(user_id) if user_id == message.sender => {
                NotificationTarget::DirectMessage { user_id }
            }
            Listener::User(user_id) => {
                if !subscribes_others(message) {
                    return Err("Only moderators can subscribe someone else. Use `tell me` to subscribe yourself.".to_owned());
                }
                NotificationTarget::DirectMessage { user_id }
            }
        };
        Ok(NewSubscription {
            requester: message.sender,
            league,
            source: tell
                .player
                .map_or(SubscriptionSource::League, SubscriptionSource::Player),
            event: tell.event,
            target,
        })
    }

    async fn subscribe(
        &mut self,
        message: &IncomingMessage,
        subscription: NewSubscription,
    ) -> Result<String, SubscriptionsError> {
        let description = describe(
            message.sender,
            &subscription.league,
            &subscription.source,
            subscription.event,
            &subscription.target,
        );
        let earlier = match message.revision {
            Revision::Original => None,
            Revision::Edited => self.subscribed_by.get(&message.id),
        };
        if let Some((id, _)) = earlier.filter(|(_, earlier)| *earlier == subscription) {
            return Ok(format!("Subscription `{id}`: {description}."));
        }
        let earlier = earlier.map(|(id, _)| *id);
        let id = self.subscriptions.subscribe(subscription.clone()).await?;
        let replaced = match earlier.filter(|earlier| *earlier != id) {
            Some(earlier) => self
                .subscriptions
                .remove(earlier, remover(message))
                .await?
                .then_some(earlier),
            None => None,
        };
        self.subscribed_by.insert(message.id, (id, subscription));
        let replacement = replaced
            .map(|earlier| format!(" It replaces subscription `{earlier}`."))
            .unwrap_or_default();
        Ok(format!("Subscription `{id}`: {description}.{replacement}"))
    }

    async fn list(&self, message: &IncomingMessage) -> Result<String, SubscriptionsError> {
        let (subscriptions, heading, none) = match listing(message) {
            Listing::Topic(topic) => (
                self.subscriptions.posting_to(topic).await?,
                "Subscriptions that post in this topic:",
                "No subscription posts in this topic.",
            ),
            Listing::Mine(user_id) => {
                let mut yours = self.subscriptions.requested_by(user_id).await?;
                yours.extend(
                    self.subscriptions
                        .posting_to(&NotificationTarget::DirectMessage { user_id })
                        .await?,
                );
                yours.sort_by_key(|subscription| subscription.id);
                yours.dedup_by_key(|subscription| subscription.id);
                (yours, "Your subscriptions:", "You have no subscriptions.")
            }
        };
        if subscriptions.is_empty() {
            return Ok(none.to_owned());
        }
        let lines: Vec<String> = subscriptions
            .iter()
            .map(|subscription| {
                let made_by = if subscription.requester == message.sender {
                    String::new()
                } else {
                    format!(", made by {}", mention(subscription.requester))
                };
                format!(
                    "- `{}`: {}{made_by}",
                    subscription.id,
                    describe(
                        message.sender,
                        &subscription.league,
                        &subscription.source,
                        subscription.event,
                        &subscription.target
                    )
                )
            })
            .collect();
        Ok(format!("{heading}\n{}", lines.join("\n")))
    }

    async fn remove(
        &self,
        message: &IncomingMessage,
        id: SubscriptionId,
    ) -> Result<String, SubscriptionsError> {
        let remover = remover(message);
        let missing = match &remover {
            Remover::Moderator => format!("There is no subscription `{id}`."),
            Remover::Owner {
                at: NotificationTarget::Channel { .. },
                ..
            } => format!(
                "No subscription `{id}` is yours or posts in this topic. Use `subscription list` to see the ones here."
            ),
            Remover::Owner {
                at: NotificationTarget::DirectMessage { .. },
                ..
            } => format!("You have no subscription `{id}`. Use `subscription list` to see yours."),
        };
        if self.subscriptions.remove(id, remover).await? {
            return Ok(format!("Removed subscription `{id}`."));
        }
        Ok(missing)
    }

    fn permissions(message: &IncomingMessage) -> String {
        let removing = match (remover(message), listing(message)) {
            (Remover::Moderator, _) => {
                "You are a moderator, so you can remove any subscription from anywhere."
            }
            (Remover::Owner { .. }, Listing::Topic(_)) => {
                "In this topic you can list and remove any subscription that posts here. You can remove the ones you made from anywhere."
            }
            (Remover::Owner { .. }, Listing::Mine(_)) => {
                "You can list and remove the subscriptions you made and the ones that send you direct messages."
            }
        };
        if subscribes_others(message) {
            format!(
                "{removing} You can also subscribe someone else with `tell @**user** when <event> in <league>`."
            )
        } else {
            removing.to_owned()
        }
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

    fn help(&self, message: &IncomingMessage) -> String {
        format!(
            "I know these commands:\n\
             - `tell me when <event> in <league>` sends you a direct message about every game in the league.\n\
             - `tell here when <event> in <league>` posts about every game in the topic where you send it.\n\
             - `tell #**channel>topic** when <event> in <league>` posts about every game in that topic. To write the link, type `#` and pick the topic.\n\
             - Moderators can send `tell @**user** when <event> in <league>` to send that user a direct message about every game in the league.\n\
             - Add `for <lichess-player>` to the end of a `tell` command to hear about one player's games only.\n\
             - `subscription list` lists the subscriptions that post in the topic where you send it. In a direct message, it lists the ones you made and the ones that send you direct messages.\n\
             - `subscription remove <id>` removes a subscription.\n\
             \n\
             Subscriptions that post in a topic belong to everyone in that topic: anyone can list or remove them by sending the command there.\n\
             `<event>` is `{STARTS}` or `{IS_OVER}`, and plain words such as `a game ends` work too. `<league>` is a league's tag, name, or time control. {}{}\n\
             {}",
            self.active_leagues(),
            self.example(),
            Self::permissions(message)
        )
    }

    fn explain(&self, message: &IncomingMessage, mistake: Mistake) -> String {
        match mistake {
            Mistake::Tell(problem, draft) => self.correct(message, problem, &draft),
            Mistake::Subscription => "Use `tell me when <event> in <league>` to add a subscription, `subscription list` to see yours, or `subscription remove <id>` to remove one.".to_owned(),
            Mistake::Id => {
                "A subscription id is a number. Use `subscription list` to see yours.".to_owned()
            }
        }
    }

    fn correct(&self, message: &IncomingMessage, problem: Problem, draft: &Draft) -> String {
        let reason = match problem {
            Problem::Wording => "Write it as `tell me when <event> in <league>`. Add `for <lichess-player>` to hear about one player's games only.".to_owned(),
            Problem::Listener => "You can tell `me`, `here`, or a topic, such as `#**channel>topic**`.".to_owned(),
            Problem::TopicLink => "A topic link looks like `#**channel>topic**`. To write one, type `#` and pick the topic.".to_owned(),
            Problem::Person => "I could not tell who you mentioned. Type `@` and pick them from the list.".to_owned(),
            Problem::Event => format!("`<event>` is `{STARTS}` or `{IS_OVER}`."),
            Problem::League => format!(
                "`<league>` is a league's tag, name, or time control. {}",
                self.active_leagues()
            ),
            Problem::Player => "That is not a Lichess username.".to_owned(),
        };
        let league = match draft
            .league
            .as_ref()
            .map(|written| league_lookup::find(&self.active, written))
        {
            Some(Found::One(league)) => Some(league),
            Some(Found::Several(_) | Found::Nothing) | None => None,
        };
        format!(
            "{reason}\nTry `{}`.",
            suggestion(message, problem, draft, league)
        )
    }

    fn unknown_league(
        &self,
        message: &IncomingMessage,
        draft: &Draft,
        found: &Found<'_>,
    ) -> String {
        let written = draft
            .league
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default();
        match found {
            Found::Several(leagues) => {
                let tags: Vec<String> = leagues
                    .iter()
                    .map(|league| format!("`{}`", league.tag))
                    .collect();
                let tries: Vec<String> = leagues
                    .iter()
                    .map(|league| {
                        format!(
                            "`{}`",
                            suggestion(message, Problem::League, draft, Some(league))
                        )
                    })
                    .collect();
                format!(
                    "`{written}` could be {}.\nTry {}.",
                    tags.join(" or "),
                    tries.join(" or ")
                )
            }
            Found::One(_) | Found::Nothing => format!(
                "`{written}` is not an active league. {}",
                self.active_leagues()
            ),
        }
    }
}

enum Listing<'a> {
    Topic(&'a NotificationTarget),
    Mine(ChatUserId),
}

const fn listing(message: &IncomingMessage) -> Listing<'_> {
    match &message.reply_to {
        topic @ NotificationTarget::Channel { .. } => Listing::Topic(topic),
        NotificationTarget::DirectMessage { .. } => Listing::Mine(message.sender),
    }
}

fn remover(message: &IncomingMessage) -> Remover {
    if message.role.moderates() {
        Remover::Moderator
    } else {
        Remover::Owner {
            user: message.sender,
            at: message.reply_to.clone(),
        }
    }
}

const fn subscribes_others(message: &IncomingMessage) -> bool {
    message.role.moderates()
}

fn mention(user: ChatUserId) -> String {
    format!("@_**|{}**", user.0)
}

fn suggestion(
    message: &IncomingMessage,
    problem: Problem,
    draft: &Draft,
    league: Option<&League>,
) -> String {
    let listener = match &draft.listener {
        Some(Listener::Me) => "me".to_owned(),
        Some(Listener::Here) => "here".to_owned(),
        Some(Listener::Topic { channel, topic }) => {
            format!("#**{}>{}**", channel.as_str(), topic.as_str())
        }
        Some(Listener::User(user_id)) => mention(*user_id),
        None => match message.reply_to {
            NotificationTarget::Channel { .. } => "here".to_owned(),
            NotificationTarget::DirectMessage { .. } => "me".to_owned(),
        },
    };
    let event = match draft.event {
        Some(GameEventKind::GameFinished) => IS_OVER,
        Some(GameEventKind::GameStarted) | None => STARTS,
    };
    let league = league.map_or_else(|| "<league>".to_owned(), |league| league.tag.to_string());
    let player = match (&draft.player, problem) {
        (Some(player), _) => format!(" for {player}"),
        (None, Problem::Player) => " for <lichess-player>".to_owned(),
        (None, _) => String::new(),
    };
    format!("tell {listener} when {event} in {league}{player}")
}

fn describe(
    viewer: ChatUserId,
    league: &league::Tag,
    source: &SubscriptionSource,
    event: GameEventKind,
    target: &NotificationTarget,
) -> String {
    let listener = match target {
        NotificationTarget::DirectMessage { user_id } if *user_id == viewer => "you".to_owned(),
        NotificationTarget::DirectMessage { user_id } => mention(*user_id),
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

impl<R: Subscriptions> Actor for SubscriptionCommands<R> {
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

impl<R: Subscriptions> Message<ActiveLeagues> for SubscriptionCommands<R> {
    type Reply = ();

    async fn handle(&mut self, active: ActiveLeagues, _: &mut Context<Self, Self::Reply>) {
        self.active = active;
    }
}

impl<R: Subscriptions> Message<IncomingMessage> for SubscriptionCommands<R> {
    type Reply = Handling;

    async fn handle(
        &mut self,
        message: IncomingMessage,
        _: &mut Context<Self, Self::Reply>,
    ) -> Handling {
        let reply = match parse(&message.text, &self.lichess) {
            Some(command) => self.reply_to(&message, command).await,
            None => match self.earlier_subscription(&message) {
                Some(id) => format!(
                    "Subscription `{id}` is unchanged. Send `help` to see the commands I know."
                ),
                None => return Handling::Unrecognized,
            },
        };
        Handling::Answered(Notification::new(reply))
    }
}

#[cfg(test)]
mod tests;
