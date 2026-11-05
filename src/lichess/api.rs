mod games;
mod stream;
mod token;

use reqwest::RequestBuilder;
use secrecy::{ExposeSecret, SecretString};
use url::Url;

pub use games::{GamesUserError, GamesUserQuery};
pub use stream::{FeedError, FeedEvent, GameFeed};
pub use token::TokenInfo;

#[derive(Clone, Debug)]
pub struct Api {
    http: reqwest::Client,
    site: Url,
    token: Option<SecretString>,
}

impl Api {
    #[must_use]
    pub fn new(http: reqwest::Client, site: &Url, token: Option<SecretString>) -> Self {
        Self {
            http,
            site: site.clone(),
            token,
        }
    }

    fn authorized(&self, request: RequestBuilder) -> RequestBuilder {
        match &self.token {
            Some(token) => request.bearer_auth(token.expose_secret()),
            None => request,
        }
    }
}

#[cfg(test)]
mod tests;
