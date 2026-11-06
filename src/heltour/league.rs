use nutype::nutype;
use serde::Deserialize;

use crate::game_monitor;
use crate::lichess;

const MONITORED_SOURCE: &str = "heltour";

/// ```
/// use arboters::heltour::league::Tag;
///
/// assert_eq!(Tag::try_new("Team4545").unwrap().as_str(), "team4545");
/// assert!(Tag::try_new("lone wolf").is_err());
/// ```
#[nutype(
    sanitize(trim, lowercase),
    validate(predicate = is_slug),
    derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deref, Display, Deserialize),
)]
pub struct Tag(String);

impl Tag {
    #[must_use]
    pub fn monitored(&self) -> game_monitor::League {
        game_monitor::League {
            source: game_monitor::league::Source::new(MONITORED_SOURCE),
            key: game_monitor::league::Key::new(self.as_str()),
        }
    }

    #[must_use]
    pub fn of_monitored(league: &game_monitor::League) -> Option<Self> {
        if league.source.as_str() != MONITORED_SOURCE {
            return None;
        }
        Self::try_new(league.key.as_str())
            .inspect_err(|error| {
                tracing::error!(error = %error, league = %league, "a heltour league's key is not a heltour tag");
            })
            .ok()
    }
}

fn is_slug(candidate: &str) -> bool {
    !candidate.is_empty()
        && candidate
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

#[nutype(
    sanitize(trim),
    validate(not_empty),
    derive(Debug, Clone, PartialEq, Eq, Deref, Display, Deserialize)
)]
pub struct Name(String);

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct League {
    pub tag: Tag,
    pub name: Name,
    pub time_control: Option<lichess::Clock>,
    pub variant: lichess::Variant,
    pub rated: bool,
}

#[cfg(test)]
mod tests;
