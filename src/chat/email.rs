use nutype::nutype;

/// ```
/// use arboters::chat::Email;
///
/// assert_eq!(Email::try_new(" alice@example.com ").unwrap().as_str(), "alice@example.com");
/// assert!(Email::try_new("alice").is_err());
/// assert!(Email::try_new("@example.com").is_err());
/// ```
#[nutype(
    sanitize(trim),
    validate(predicate = is_email),
    derive(Debug, Clone, PartialEq, Eq, Deref, Deserialize)
)]
pub struct Email(String);

fn is_email(candidate: &str) -> bool {
    candidate
        .split_once('@')
        .is_some_and(|(local, domain)| !local.is_empty() && !domain.is_empty())
}
