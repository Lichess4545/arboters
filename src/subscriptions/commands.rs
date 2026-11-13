mod league_lookup;
mod parsing;
mod spelling;

use std::collections::HashMap;
use std::convert::Infallible;

use kameo::Actor;
use kameo::actor::{ActorRef, Spawn};
use kameo::message::{Context, Message};
use tokio::sync::watch;
use url::Url;

use crate::chat::{
    ChannelName, ChatUserId, Conversation, IncomingMessage, MessageId, OutgoingMessage, Revision,
    Role, TopicName,
};
use crate::commands::{Handling, Listener};
use crate::game_monitor::GameEventKind;
use crate::heltour::{ActiveLeagues, League, league};

use super::model::{NewSubscription, Remover, SubscriptionId, SubscriptionSource};
use super::{Subscriptions, SubscriptionsError};
use parsing::{Audience, Draft, IS_OVER, Mistake, Problem, STARTS, Tell, parse};
use spelling::Found;

#[derive(Debug)]
pub struct SubscriptionCommands<R> {
    active_leagues: watch::Receiver<ActiveLeagues>,
    active: ActiveLeagues,
    subscriptions: R,
    subscribed_by: HashMap<MessageId, (SubscriptionId, NewSubscription)>,
    lichess: Url,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Help,
    Tell(Tell),
    List,
    Remove(SubscriptionId),
    Mistake(Mistake),
    EditedAway,
}

#[derive(Debug)]
pub struct SubscriptionCommand {
    asker: Asker,
    action: Action,
}

#[derive(Debug)]
struct Asker {
    sender: ChatUserId,
    role: Role,
    reply_to: Conversation,
    message: MessageId,
    revision: Revision,
}

impl SubscriptionCommand {
    fn new(message: &IncomingMessage, action: Option<Action>) -> Option<Self> {
        let action = match (action, message.revision) {
            (Some(action), _) => action,
            (None, Revision::Edited) => Action::EditedAway,
            (None, Revision::Original) => return None,
        };
        Some(Self {
            asker: Asker {
                sender: message.sender,
                role: message.role,
                reply_to: message.reply_to.clone(),
                message: message.id,
                revision: message.revision,
            },
            action,
        })
    }
}

impl<R: Subscriptions> SubscriptionCommands<R> {
    #[must_use]
    pub fn new(
        subscriptions: R,
        lichess: Url,
        active_leagues: watch::Receiver<ActiveLeagues>,
    ) -> Self {
        Self {
            active_leagues,
            active: ActiveLeagues::default(),
            subscriptions,
            subscribed_by: HashMap::new(),
            lichess,
        }
    }

    fn earlier_subscription(&self, asker: &Asker) -> Option<SubscriptionId> {
        match asker.revision {
            Revision::Original => None,
            Revision::Edited => self.subscribed_by.get(&asker.message).map(|(id, _)| *id),
        }
    }

    async fn answer(&mut self, asker: &Asker, action: Action) -> Option<String> {
        let outcome = match action {
            Action::Help => Ok(self.help(asker)),
            Action::Tell(tell) => match self.request(asker, tell) {
                Ok(subscription) => self.subscribe(asker, subscription).await,
                Err(refusal) => Ok(self.unchanged(asker, refusal)),
            },
            Action::List => self.list(asker).await,
            Action::Remove(id) => self.remove(asker, id).await,
            Action::Mistake(mistake) => Ok(self.unchanged(asker, self.explain(asker, mistake))),
            Action::EditedAway => {
                return self.earlier_subscription(asker).map(|id| {
                    format!(
                        "Subscription `{id}` is unchanged. Send `help` to see the commands I know."
                    )
                });
            }
        };
        Some(outcome.unwrap_or_else(|error| {
            tracing::error!(error = %error, cause = ?std::error::Error::source(&error), "subscription command failed");
            "Something went wrong on my side. Please try again later.".to_owned()
        }))
    }

    fn unchanged(&self, asker: &Asker, reply: String) -> String {
        match self.earlier_subscription(asker) {
            Some(id) => format!("{reply}\nSubscription `{id}` is unchanged."),
            None => reply,
        }
    }

    fn request(&self, asker: &Asker, tell: Tell) -> Result<NewSubscription, String> {
        let league = match league_lookup::find(&self.active, &tell.league) {
            Found::One(league) => league.tag.clone(),
            found => return Err(self.unknown_league(asker, &Draft::from(tell), &found)),
        };
        let target = match tell.audience {
            Audience::Me => Conversation::Direct {
                user_id: asker.sender,
            },
            Audience::Here => match &asker.reply_to {
                Conversation::Direct { .. } => {
                    return Err("`tell here` only works in a channel topic. In a direct message, use `tell me`, or name a topic with `tell #**channel>topic**`.".to_owned());
                }
                channel @ Conversation::Topic { .. } => channel.clone(),
            },
            Audience::Topic { channel, topic } => Conversation::Topic { channel, topic },
            Audience::User(user_id) if user_id != asker.sender && !asker.role.moderates() => {
                return Err("Only moderators can subscribe someone else. Use `tell me` to subscribe yourself.".to_owned());
            }
            Audience::User(user_id) => Conversation::Direct { user_id },
        };
        Ok(NewSubscription {
            requester: asker.sender,
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
        asker: &Asker,
        subscription: NewSubscription,
    ) -> Result<String, SubscriptionsError> {
        let description = describe(
            asker.sender,
            &subscription.league,
            &subscription.source,
            subscription.event,
            &subscription.target,
        );
        let earlier = match asker.revision {
            Revision::Original => None,
            Revision::Edited => self.subscribed_by.get(&asker.message),
        };
        if let Some((id, _)) = earlier.filter(|(_, earlier)| *earlier == subscription) {
            return Ok(format!("Subscription `{id}`: {description}."));
        }
        let earlier = earlier.map(|(id, _)| *id);
        let id = self.subscriptions.subscribe(subscription.clone()).await?;
        let replaced = match earlier.filter(|earlier| *earlier != id) {
            Some(earlier) => self
                .subscriptions
                .remove(earlier, remover(asker))
                .await?
                .then_some(earlier),
            None => None,
        };
        self.subscribed_by.insert(asker.message, (id, subscription));
        let replacement = replaced
            .map(|earlier| format!(" It replaces subscription `{earlier}`."))
            .unwrap_or_default();
        Ok(format!("Subscription `{id}`: {description}.{replacement}"))
    }

    async fn list(&self, asker: &Asker) -> Result<String, SubscriptionsError> {
        let subscriptions = self.subscriptions.listed_in(&asker.reply_to).await?;
        let (heading, none) = match asker.reply_to {
            Conversation::Topic { .. } => (
                "Subscriptions that post in this topic:",
                "No subscription posts in this topic.",
            ),
            Conversation::Direct { .. } => ("Your subscriptions:", "You have no subscriptions."),
        };
        if subscriptions.is_empty() {
            return Ok(none.to_owned());
        }
        let lines: Vec<String> = subscriptions
            .iter()
            .map(|subscription| {
                let made_by = if subscription.requester == asker.sender {
                    String::new()
                } else {
                    format!(", made by {}", mention(subscription.requester))
                };
                format!(
                    "- `{}`: {}{made_by}",
                    subscription.id,
                    describe(
                        asker.sender,
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
        asker: &Asker,
        id: SubscriptionId,
    ) -> Result<String, SubscriptionsError> {
        let remover = remover(asker);
        let missing = match &remover {
            Remover::Moderator => format!("There is no subscription `{id}`."),
            Remover::Owner {
                at: Conversation::Topic { .. },
                ..
            } => format!(
                "No subscription `{id}` is yours or posts in this topic. Use `subscription list` to see the ones here."
            ),
            Remover::Owner {
                at: Conversation::Direct { .. },
                ..
            } => format!("You have no subscription `{id}`. Use `subscription list` to see yours."),
        };
        if self.subscriptions.remove(id, remover).await? {
            return Ok(format!("Removed subscription `{id}`."));
        }
        Ok(missing)
    }

    fn permissions(asker: &Asker) -> String {
        let removing = match (remover(asker), &asker.reply_to) {
            (Remover::Moderator, _) => {
                "You are a moderator, so you can remove any subscription from anywhere."
            }
            (Remover::Owner { .. }, Conversation::Topic { .. }) => {
                "In this topic you can list and remove any subscription that posts here. You can remove the ones you made from anywhere."
            }
            (Remover::Owner { .. }, Conversation::Direct { .. }) => {
                "You can list and remove the subscriptions you made and the ones that send you direct messages."
            }
        };
        if asker.role.moderates() {
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

    fn help(&self, asker: &Asker) -> String {
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
            Self::permissions(asker)
        )
    }

    fn explain(&self, asker: &Asker, mistake: Mistake) -> String {
        match mistake {
            Mistake::Tell(problem, draft) => self.correct(asker, problem, &draft),
            Mistake::Subscription => "Use `tell me when <event> in <league>` to add a subscription, `subscription list` to see yours, or `subscription remove <id>` to remove one.".to_owned(),
            Mistake::Id => {
                "A subscription id is a number. Use `subscription list` to see yours.".to_owned()
            }
        }
    }

    fn correct(&self, asker: &Asker, problem: Problem, draft: &Draft) -> String {
        let reason = match problem {
            Problem::Wording => "Write it as `tell me when <event> in <league>`. Add `for <lichess-player>` to hear about one player's games only.".to_owned(),
            Problem::Audience => "You can tell `me`, `here`, or a topic, such as `#**channel>topic**`.".to_owned(),
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
            suggestion(asker, problem, draft, league)
        )
    }

    fn unknown_league(&self, asker: &Asker, draft: &Draft, found: &Found<&League>) -> String {
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
                            suggestion(asker, Problem::League, draft, Some(league))
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

fn remover(asker: &Asker) -> Remover {
    if asker.role.moderates() {
        Remover::Moderator
    } else {
        Remover::Owner {
            user: asker.sender,
            at: asker.reply_to.clone(),
        }
    }
}

fn mention(user: ChatUserId) -> String {
    format!("@_**|{}**", user.0)
}

fn topic_link(channel: &ChannelName, topic: &TopicName) -> String {
    format!("#**{}>{}**", channel.as_str(), topic.as_str())
}

const fn event_name(event: GameEventKind) -> &'static str {
    match event {
        GameEventKind::GameStarted => STARTS,
        GameEventKind::GameFinished => IS_OVER,
    }
}

fn suggestion(asker: &Asker, problem: Problem, draft: &Draft, league: Option<&League>) -> String {
    let audience = match &draft.audience {
        Some(Audience::Me) => "me".to_owned(),
        Some(Audience::Here) => "here".to_owned(),
        Some(Audience::Topic { channel, topic }) => topic_link(channel, topic),
        Some(Audience::User(user_id)) => mention(*user_id),
        None => match asker.reply_to {
            Conversation::Topic { .. } => "here".to_owned(),
            Conversation::Direct { .. } => "me".to_owned(),
        },
    };
    let event = event_name(draft.event.unwrap_or(GameEventKind::GameStarted));
    let league = league.map_or_else(|| "<league>".to_owned(), |league| league.tag.to_string());
    let player = match (&draft.player, problem) {
        (Some(player), _) => format!(" for {player}"),
        (None, Problem::Player) => " for <lichess-player>".to_owned(),
        (None, _) => String::new(),
    };
    format!("tell {audience} when {event} in {league}{player}")
}

fn describe(
    viewer: ChatUserId,
    league: &league::Tag,
    source: &SubscriptionSource,
    event: GameEventKind,
    target: &Conversation,
) -> String {
    let audience = match target {
        Conversation::Direct { user_id } if *user_id == viewer => "you".to_owned(),
        Conversation::Direct { user_id } => mention(*user_id),
        Conversation::Topic { channel, topic } => topic_link(channel, topic),
    };
    let event = event_name(event);
    match source {
        SubscriptionSource::Player(player) => {
            format!("tell {audience} when {event} in {league} for {player}")
        }
        SubscriptionSource::Team(team) => {
            format!(
                "tell {audience} when {event} in {league} for team {}",
                team.as_str()
            )
        }
        SubscriptionSource::League => format!("tell {audience} when {event} in {league}"),
    }
}

impl<R: Subscriptions> SubscriptionCommands<R> {
    pub fn listen(self) -> (ActorRef<Self>, Listener) {
        let lichess = self.lichess.clone();
        let subscription_commands = Self::spawn(self);
        let listener = Listener::new(
            move |message: &IncomingMessage| {
                SubscriptionCommand::new(message, parse(&message.text, &lichess))
            },
            subscription_commands
                .clone()
                .reply_recipient::<SubscriptionCommand>(),
        );
        (subscription_commands, listener)
    }
}

impl<R: Subscriptions> Actor for SubscriptionCommands<R> {
    type Args = Self;
    type Error = Infallible;

    fn on_start(
        subscription_commands: Self,
        _: ActorRef<Self>,
    ) -> impl Future<Output = Result<Self, Self::Error>> + Send {
        std::future::ready(Ok(subscription_commands))
    }
}

impl<R: Subscriptions> Message<SubscriptionCommand> for SubscriptionCommands<R> {
    type Reply = Handling;

    async fn handle(
        &mut self,
        SubscriptionCommand { asker, action }: SubscriptionCommand,
        _: &mut Context<Self, Self::Reply>,
    ) -> Handling {
        self.active = self.active_leagues.borrow().clone();
        self.answer(&asker, action)
            .await
            .map_or(Handling::Declined, |reply| {
                Handling::Answered(OutgoingMessage::new(reply))
            })
    }
}

#[cfg(test)]
mod tests;
