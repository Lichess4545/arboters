use crate::chat::{ChannelName, TopicName};
use crate::game_monitor::GameEventKind;
use crate::heltour::league;
use crate::lichess;
use crate::subscriptions::SubscriptionId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Help,
    Tell(Tell),
    List,
    Remove(SubscriptionId),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tell {
    pub listener: Listener,
    pub event: GameEventKind,
    pub league: league::Tag,
    pub player: Option<lichess::UserId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Listener {
    Me,
    Here,
    Topic {
        channel: ChannelName,
        topic: TopicName,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mistake {
    Tell,
    Listener,
    TopicLink,
    Event,
    League,
    Player,
    Subscription,
    Id,
}

pub const STARTS: &str = "a-game-starts";
pub const IS_OVER: &str = "a-game-is-over";
const TOPIC_LINK: &str = "#**";

pub fn parse(text: &str) -> Option<Result<Command, Mistake>> {
    let (text, link) = take_topic_link(text);
    let lowered = text.to_lowercase();
    let mut words = lowered.split_whitespace();
    match words.next()? {
        "help" => Some(Ok(Command::Help)),
        "tell" => Some(tell(words, link.as_deref()).map(Command::Tell)),
        "subscription" => Some(subscription(words)),
        _ => None,
    }
}

fn take_topic_link(text: &str) -> (String, Option<String>) {
    let Some((before, rest)) = text.split_once(TOPIC_LINK) else {
        return (text.to_owned(), None);
    };
    let Some((link, after)) = rest.split_once("**") else {
        return (text.to_owned(), None);
    };
    (
        format!("{before} {TOPIC_LINK} {after}"),
        Some(link.to_owned()),
    )
}

fn topic(link: &str) -> Result<Listener, Mistake> {
    let (channel, topic) = link.split_once('>').ok_or(Mistake::TopicLink)?;
    Ok(Listener::Topic {
        channel: ChannelName::try_new(channel).map_err(|_| Mistake::TopicLink)?,
        topic: TopicName::try_new(topic).map_err(|_| Mistake::TopicLink)?,
    })
}

fn tell<'a>(mut words: impl Iterator<Item = &'a str>, link: Option<&str>) -> Result<Tell, Mistake> {
    let listener = match words.next() {
        Some("me") => Listener::Me,
        Some("here") => Listener::Here,
        Some(TOPIC_LINK) => link.ok_or(Mistake::TopicLink).and_then(topic)?,
        Some(word) if word.starts_with(TOPIC_LINK) => return Err(Mistake::TopicLink),
        Some(_) => return Err(Mistake::Listener),
        None => return Err(Mistake::Tell),
    };
    expect(&mut words, "when")?;
    let event = match words.next() {
        Some(STARTS) => GameEventKind::GameStarted,
        Some(IS_OVER) => GameEventKind::GameFinished,
        Some(_) => return Err(Mistake::Event),
        None => return Err(Mistake::Tell),
    };
    expect(&mut words, "in")?;
    let league = words
        .next()
        .ok_or(Mistake::Tell)
        .and_then(|tag| league::Tag::try_new(tag).map_err(|_| Mistake::League))?;
    let player = match words.next() {
        None => None,
        Some("for") => Some(
            words
                .next()
                .ok_or(Mistake::Tell)
                .and_then(|name| lichess::UserId::try_new(name).map_err(|_| Mistake::Player))?,
        ),
        Some(_) => return Err(Mistake::Tell),
    };
    if words.next().is_some() {
        return Err(Mistake::Tell);
    }
    Ok(Tell {
        listener,
        event,
        league,
        player,
    })
}

fn expect<'a>(words: &mut impl Iterator<Item = &'a str>, keyword: &str) -> Result<(), Mistake> {
    if words.next() == Some(keyword) {
        Ok(())
    } else {
        Err(Mistake::Tell)
    }
}

fn subscription<'a>(mut words: impl Iterator<Item = &'a str>) -> Result<Command, Mistake> {
    let command = match (words.next(), words.next()) {
        (Some("list"), None) => Command::List,
        (Some("remove"), Some(id)) => Command::Remove(SubscriptionId(
            id.trim_start_matches('#')
                .parse()
                .map_err(|_| Mistake::Id)?,
        )),
        _ => return Err(Mistake::Subscription),
    };
    if words.next().is_some() {
        return Err(Mistake::Subscription);
    }
    Ok(command)
}

#[cfg(test)]
mod tests;
