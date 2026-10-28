use secrecy::SecretString;
use serde::Deserialize;
use url::Url;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct BotEmail(String);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{0}` is not an email address")]
pub struct InvalidBotEmail(String);

impl BotEmail {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for BotEmail {
    type Error = InvalidBotEmail;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        match raw.split_once('@') {
            Some((local, domain)) if !local.is_empty() && !domain.is_empty() => Ok(Self(raw)),
            _ => Err(InvalidBotEmail(raw)),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ZulipSettings {
    pub site: Url,
    pub email: BotEmail,
    pub api_key: SecretString,
}
