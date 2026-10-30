use nutype::nutype;
use serde::Deserialize;

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

fn is_slug(candidate: &str) -> bool {
    !candidate.is_empty()
        && candidate
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct League {
    pub tag: Tag,
}

#[cfg(test)]
mod tests;
