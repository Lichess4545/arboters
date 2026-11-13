use url::Url;

use super::Action;
use crate::responses::ResponseId;
use crate::responses::link::{LinkMistake, linked_message};
use crate::responses::phrase::{Matching, Phrase};

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

pub fn parse(text: &str, site: &Url) -> Option<Action> {
    let (first, rest) = first_word(text);
    match first.to_lowercase().as_str() {
        "help" => Some(Action::Help),
        "response" | "responses" => Some(response(rest, site)),
        _ => None,
    }
}

fn first_word(text: &str) -> (&str, &str) {
    let text = text.trim();
    text.split_once(char::is_whitespace)
        .map_or((text, ""), |(word, rest)| (word, rest.trim()))
}

fn response(text: &str, site: &Url) -> Action {
    let (verb, rest) = first_word(text);
    match verb.to_lowercase().as_str() {
        "" | "help" if rest.is_empty() => Action::ResponseHelp,
        "add" => add(rest, site).unwrap_or_else(Action::Mistake),
        "list" if rest.is_empty() => Action::List(None),
        "list" => match quoted(rest) {
            Some((inside, "")) => {
                phrase(inside).map_or_else(Action::Mistake, |phrase| Action::List(Some(phrase)))
            }
            _ => Action::Mistake(Mistake::List),
        },
        "remove" => rest
            .trim_start_matches('#')
            .parse()
            .map_or(Action::Mistake(Mistake::Remove), |id| {
                Action::Remove(ResponseId(id))
            }),
        _ => Action::Mistake(Mistake::Unknown),
    }
}

fn add(text: &str, site: &Url) -> Result<Action, Mistake> {
    let (matching, text) = match first_word(text) {
        (word, rest) if word.eq_ignore_ascii_case("exact") => (Matching::ExactCase, rest),
        _ => (Matching::AnyCase, text),
    };
    let (inside, link) = quoted(text).ok_or(Mistake::Add)?;
    let phrase = phrase(inside)?;
    if link.is_empty() {
        return Err(Mistake::Add);
    }
    Ok(Action::Add {
        phrase,
        matching,
        source: linked_message(link, site).map_err(Mistake::Link)?,
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
