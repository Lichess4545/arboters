use nutype::nutype;

/// ```
/// use arboters::lichess::UserId;
///
/// let id = UserId::try_new("MrScribbles").unwrap();
/// assert_eq!(id.as_str(), "mrscribbles");
/// assert_eq!(id, UserId::try_new("mrscribbles").unwrap());
/// ```
#[nutype(
    sanitize(trim, lowercase),
    validate(predicate = is_user_id),
    derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deref, Display, Deserialize),
)]
pub struct UserId(String);

fn is_user_id(candidate: &str) -> bool {
    (2..=30).contains(&candidate.len())
        && candidate
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

#[cfg(test)]
mod tests;
