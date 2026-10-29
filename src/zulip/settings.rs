use std::time::Duration;

use nutype::nutype;
use secrecy::SecretString;
use serde::Deserialize;
use url::Url;

#[nutype(
    validate(predicate = is_email),
    derive(Debug, Clone, PartialEq, Eq, Deref, Deserialize)
)]
pub struct BotEmail(String);

fn is_email(candidate: &str) -> bool {
    candidate
        .split_once('@')
        .is_some_and(|(local, domain)| !local.is_empty() && !domain.is_empty())
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ZulipSettings {
    pub site: Url,
    pub email: BotEmail,
    pub api_key: SecretString,
    #[serde(default)]
    pub inbox: InboxSettings,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct InboxSettings {
    #[serde(with = "humantime_serde")]
    pub initial_backoff: Duration,
    #[serde(with = "humantime_serde")]
    pub max_backoff: Duration,
}

impl Default for InboxSettings {
    fn default() -> Self {
        Self {
            initial_backoff: Duration::from_secs(1),
            max_backoff: Duration::from_mins(1),
        }
    }
}
