use nutype::nutype;

use crate::chat::{ChannelName, ChatUserId, TopicName};
use crate::game_monitor::GameEventKind;
use crate::lichess;
use crate::subscriptions::SubscriptionId;

use super::spelling::{Closest, closest, matches};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Help,
    Tell(Tell),
    List,
    Remove(SubscriptionId),
}

#[nutype(
    sanitize(with = |written: String| written.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()),
    validate(not_empty),
    derive(Debug, Clone, PartialEq, Eq, Deref, Display)
)]
pub struct WrittenLeague(String);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tell {
    pub listener: Listener,
    pub event: GameEventKind,
    pub league: WrittenLeague,
    pub player: Option<lichess::UserId>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Draft {
    pub listener: Option<Listener>,
    pub event: Option<GameEventKind>,
    pub league: Option<WrittenLeague>,
    pub player: Option<lichess::UserId>,
}

impl From<Tell> for Draft {
    fn from(tell: Tell) -> Self {
        Self {
            listener: Some(tell.listener),
            event: Some(tell.event),
            league: Some(tell.league),
            player: tell.player,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Listener {
    Me,
    Here,
    Topic {
        channel: ChannelName,
        topic: TopicName,
    },
    User(ChatUserId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Problem {
    Wording,
    Listener,
    TopicLink,
    Person,
    Event,
    League,
    Player,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Mistake {
    Tell(Problem, Draft),
    Subscription,
    Id,
}

pub const STARTS: &str = "a-game-starts";
pub const IS_OVER: &str = "a-game-is-over";
const TOPIC_LINK: &str = "#**";
const MENTION: &str = "@**";
const SILENT_MENTION: &str = "@_**";

const ARTICLES: [&str; 2] = ["a", "the"];
const EVENT_WORDS: [&str; 7] = ["game", "starts", "begins", "is", "over", "ends", "finishes"];
const EVENTS: [(&[&str], GameEventKind); 11] = [
    (&["game", "starts"], GameEventKind::GameStarted),
    (&["starts"], GameEventKind::GameStarted),
    (&["game", "begins"], GameEventKind::GameStarted),
    (&["begins"], GameEventKind::GameStarted),
    (&["game", "is", "over"], GameEventKind::GameFinished),
    (&["is", "over"], GameEventKind::GameFinished),
    (&["game", "over"], GameEventKind::GameFinished),
    (&["game", "ends"], GameEventKind::GameFinished),
    (&["ends"], GameEventKind::GameFinished),
    (&["game", "finishes"], GameEventKind::GameFinished),
    (&["finishes"], GameEventKind::GameFinished),
];
const LICHESS_PROFILE: &str = "lichess.org/@/";

#[derive(Clone, Copy)]
enum Verb {
    Help,
    Tell,
    Subscription,
}

pub fn parse(text: &str) -> Option<Result<Command, Mistake>> {
    let (text, link) = take_enclosed(text, TOPIC_LINK);
    let (text, mention) = take_enclosed(&text.replacen(SILENT_MENTION, MENTION, 1), MENTION);
    let lowered = text.to_lowercase();
    let words: Vec<&str> = lowered.split_whitespace().collect();
    let (verb, rest) = words.split_first()?;
    let verbs = [
        ("help", Verb::Help),
        ("tell", Verb::Tell),
        ("subscription", Verb::Subscription),
    ];
    match closest(verb, verbs) {
        Closest::One(Verb::Help) => Some(Ok(Command::Help)),
        Closest::One(Verb::Tell) => {
            Some(tell(rest, link.as_deref(), mention.as_deref()).map(Command::Tell))
        }
        Closest::One(Verb::Subscription) => Some(subscription(rest)),
        Closest::Tie(_) | Closest::Nothing => None,
    }
}

fn take_enclosed(text: &str, opener: &str) -> (String, Option<String>) {
    let Some((before, rest)) = text.split_once(opener) else {
        return (text.to_owned(), None);
    };
    let Some((inside, after)) = rest.split_once("**") else {
        return (text.to_owned(), None);
    };
    (
        format!("{before} {opener} {after}"),
        Some(inside.to_owned()),
    )
}

fn person(mention: &str) -> Result<Listener, Problem> {
    mention
        .rsplit_once('|')
        .and_then(|(_, id)| id.trim().parse().ok())
        .map(|id| Listener::User(ChatUserId(id)))
        .ok_or(Problem::Person)
}

fn topic(link: &str) -> Result<Listener, Problem> {
    let (channel, topic) = link.split_once('>').ok_or(Problem::TopicLink)?;
    Ok(Listener::Topic {
        channel: ChannelName::try_new(channel).map_err(|_| Problem::TopicLink)?,
        topic: TopicName::try_new(topic).map_err(|_| Problem::TopicLink)?,
    })
}

struct Sections<'a> {
    listener: &'a [&'a str],
    event: Option<&'a [&'a str]>,
    league: Option<&'a [&'a str]>,
    player: Option<&'a [&'a str]>,
}

fn sections<'a>(words: &'a [&'a str]) -> Sections<'a> {
    let after = |start: usize, keyword: &dyn Fn(&str) -> bool| {
        words
            .iter()
            .enumerate()
            .skip(start)
            .find_map(|(index, word)| keyword(word).then_some(index))
    };
    let when = after(0, &|word| matches(word, "when"));
    let event_start = when.map_or(0, |when| when + 1);
    let r#in = after(event_start, &|word| word == "in");
    let league_start = r#in.map_or(event_start, |r#in| r#in + 1);
    let r#for = after(league_start, &|word| matches(word, "for"));
    let between = |start: Option<usize>, ends: &[Option<usize>]| {
        let start = start?;
        let end = ends.iter().flatten().next().copied().unwrap_or(words.len());
        words.get(start..end)
    };
    Sections {
        listener: between(Some(0), &[when, r#in, r#for]).unwrap_or_default(),
        event: between(when.map(|when| when + 1), &[r#in, r#for]),
        league: between(r#in.map(|r#in| r#in + 1), &[r#for]),
        player: r#for.and_then(|r#for| words.get(r#for + 1..)),
    }
}

fn tell(words: &[&str], link: Option<&str>, mention: Option<&str>) -> Result<Tell, Mistake> {
    if words.is_empty() {
        return Err(Mistake::Tell(Problem::Wording, Draft::default()));
    }
    let sections = sections(words);
    let mut draft = Draft::default();
    let mut problems = Vec::new();

    let listener_rest = match take_listener(sections.listener, link, mention) {
        Ok((listener, rest)) => {
            draft.listener = Some(listener);
            rest
        }
        Err(problem) => {
            problems.push(problem);
            &[]
        }
    };
    let event_words = continued(
        sections.event,
        listener_rest,
        Problem::Wording,
        &mut problems,
    );
    let event_rest = if let Some((event, rest)) = take_event(event_words) {
        draft.event = Some(event);
        rest
    } else {
        problems.push(Problem::Event);
        &[]
    };
    let league_words = continued(sections.league, event_rest, Problem::Event, &mut problems);
    draft.league = WrittenLeague::try_new(league_words.join(" ")).ok();
    if draft.league.is_none() {
        problems.push(Problem::League);
    }
    if let Some(player_words) = sections.player {
        match player_words {
            [written] => match player(written) {
                Some(player) => draft.player = Some(player),
                None => problems.push(Problem::Player),
            },
            _ => problems.push(Problem::Player),
        }
    }

    match (problems.first().copied(), draft) {
        (
            None,
            Draft {
                listener: Some(listener),
                event: Some(event),
                league: Some(league),
                player,
            },
        ) => Ok(Tell {
            listener,
            event,
            league,
            player,
        }),
        (problem, draft) => Err(Mistake::Tell(problem.unwrap_or(Problem::Wording), draft)),
    }
}

fn continued<'a>(
    section: Option<&'a [&'a str]>,
    leftover: &'a [&'a str],
    stray: Problem,
    problems: &mut Vec<Problem>,
) -> &'a [&'a str] {
    match (section, leftover.is_empty()) {
        (Some(section), true) => section,
        (Some(section), false) => {
            problems.push(stray);
            section
        }
        (None, true) => leftover,
        (None, false) => {
            problems.push(Problem::Wording);
            leftover
        }
    }
}

fn take_listener<'a>(
    words: &'a [&'a str],
    link: Option<&str>,
    mention: Option<&str>,
) -> Result<(Listener, &'a [&'a str]), Problem> {
    match words {
        [TOPIC_LINK, rest @ ..] => Ok((link.ok_or(Problem::TopicLink).and_then(topic)?, rest)),
        [MENTION, rest @ ..] => Ok((mention.ok_or(Problem::Person).and_then(person)?, rest)),
        [word, ..] if word.starts_with(TOPIC_LINK) => Err(Problem::TopicLink),
        ["me", rest @ ..] => Ok((Listener::Me, rest)),
        ["us", rest @ ..] | ["this", "topic", rest @ ..] => Ok((Listener::Here, rest)),
        [word, rest @ ..] if matches(word, "here") => Ok((Listener::Here, rest)),
        _ => Err(Problem::Listener),
    }
}

fn take_event<'a>(words: &'a [&'a str]) -> Option<(GameEventKind, &'a [&'a str])> {
    (1..=words.len()).rev().find_map(|length| {
        let (phrase, rest) = words.split_at_checked(length)?;
        event(phrase).map(|event| (event, rest))
    })
}

fn event(phrase: &[&str]) -> Option<GameEventKind> {
    let words: Vec<&str> = phrase
        .iter()
        .flat_map(|word| word.split('-'))
        .filter(|word| !word.is_empty() && !ARTICLES.contains(word))
        .map(
            |word| match closest(word, EVENT_WORDS.map(|known| (known, known))) {
                Closest::One(known) => known,
                Closest::Tie(_) | Closest::Nothing => word,
            },
        )
        .collect();
    EVENTS
        .iter()
        .find_map(|(known, event)| (*known == words.as_slice()).then_some(*event))
}

fn player(written: &str) -> Option<lichess::UserId> {
    let address = written
        .strip_prefix("https://")
        .or_else(|| written.strip_prefix("http://"))
        .unwrap_or(written);
    let address = address.strip_prefix("www.").unwrap_or(address);
    let name = address
        .strip_prefix(LICHESS_PROFILE)
        .map_or(address, |name| name.trim_end_matches('/'));
    let name = name.strip_prefix('@').unwrap_or(name);
    lichess::UserId::try_new(name).ok()
}

fn subscription(words: &[&str]) -> Result<Command, Mistake> {
    match words {
        [action] if matches(action, "list") => Ok(Command::List),
        [action, id] if matches(action, "remove") => Ok(Command::Remove(SubscriptionId(
            id.trim_start_matches('#')
                .parse()
                .map_err(|_| Mistake::Id)?,
        ))),
        _ => Err(Mistake::Subscription),
    }
}

#[cfg(test)]
mod tests;
