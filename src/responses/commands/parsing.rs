use url::Url;

use crate::chat::MessageId;

use crate::responses::ResponseId;
use crate::responses::link::{LinkMistake, linked_message};
use crate::responses::phrase::{Matching, Phrase};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    Help,
    Response(Result<Command, Mistake>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Help,
    Add {
        phrase: Phrase,
        matching: Matching,
        message: MessageId,
    },
    List(Option<Phrase>),
    Remove(ResponseId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mistake {
    Unknown,
    Add,
    List,
    Remove,
    EmptyPhrase,
    Link(LinkMistake),
}

const OPENING_QUOTES: [char; 2] = ['"', '\u{201c}'];
const CLOSING_QUOTES: [char; 2] = ['"', '\u{201d}'];

pub fn parse(text: &str, site: &Url) -> Option<Request> {
    let (first, rest) = first_word(text);
    match first.to_lowercase().as_str() {
        "help" => Some(Request::Help),
        "response" | "responses" => Some(Request::Response(command(rest, site))),
        _ => None,
    }
}

fn first_word(text: &str) -> (&str, &str) {
    let text = text.trim();
    text.split_once(char::is_whitespace)
        .map_or((text, ""), |(word, rest)| (word, rest.trim()))
}

fn command(text: &str, site: &Url) -> Result<Command, Mistake> {
    let (verb, rest) = first_word(text);
    match verb.to_lowercase().as_str() {
        "" | "help" if rest.is_empty() => Ok(Command::Help),
        "add" => add(rest, site),
        "list" if rest.is_empty() => Ok(Command::List(None)),
        "list" => match quoted(rest) {
            Some((inside, "")) => Ok(Command::List(Some(phrase(inside)?))),
            _ => Err(Mistake::List),
        },
        "remove" => rest
            .trim_start_matches('#')
            .parse()
            .map(|id| Command::Remove(ResponseId(id)))
            .map_err(|_| Mistake::Remove),
        _ => Err(Mistake::Unknown),
    }
}

fn add(text: &str, site: &Url) -> Result<Command, Mistake> {
    let (matching, text) = match first_word(text) {
        (word, rest) if word.eq_ignore_ascii_case("exact") => (Matching::ExactCase, rest),
        _ => (Matching::AnyCase, text),
    };
    let (inside, link) = quoted(text).ok_or(Mistake::Add)?;
    let phrase = phrase(inside)?;
    if link.is_empty() {
        return Err(Mistake::Add);
    }
    let message = linked_message(link, site).map_err(Mistake::Link)?;
    Ok(Command::Add {
        phrase,
        matching,
        message,
    })
}

fn quoted(text: &str) -> Option<(&str, &str)> {
    let (inside, after) = text
        .strip_prefix(OPENING_QUOTES)?
        .split_once(CLOSING_QUOTES)?;
    Some((inside, after.trim()))
}

fn phrase(text: &str) -> Result<Phrase, Mistake> {
    Phrase::try_new(text).map_err(|_| Mistake::EmptyPhrase)
}

#[cfg(test)]
mod tests;
