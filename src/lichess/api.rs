use std::collections::HashMap;

use reqwest::header::CONTENT_TYPE;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use url::Url;

use super::UserId;

const TOKEN_TEST_PATH: &str = "api/token/test";

#[derive(Clone, Debug)]
pub struct Api {
    http: reqwest::Client,
    site: Url,
    token: SecretString,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Owner {
    user_id: UserId,
}

impl Api {
    #[must_use]
    pub fn new(http: reqwest::Client, site: &Url, token: SecretString) -> Self {
        Self {
            http,
            site: site.clone(),
            token,
        }
    }

    #[expect(clippy::expect_used)]
    pub async fn token_owner(&self) -> reqwest::Result<Option<UserId>> {
        let owners: HashMap<String, Option<Owner>> = self
            .http
            .post(self.site.join(TOKEN_TEST_PATH).expect("constant path"))
            .header(CONTENT_TYPE, "text/plain")
            .body(self.token.expose_secret().to_owned())
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(owners
            .into_values()
            .flatten()
            .next()
            .map(|owner| owner.user_id))
    }
}
