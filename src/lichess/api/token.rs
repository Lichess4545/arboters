use std::collections::HashMap;

use reqwest::header::CONTENT_TYPE;
use secrecy::ExposeSecret;
use serde::Deserialize;

use crate::lichess::UserId;

use super::Api;

const TOKEN_TEST_PATH: &str = "api/token/test";

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenInfo {
    pub user_id: UserId,
}

impl Api {
    #[expect(clippy::expect_used)]
    pub async fn token_test(&self) -> reqwest::Result<Vec<Option<TokenInfo>>> {
        let Some(token) = &self.token else {
            return Ok(Vec::new());
        };
        let tested: HashMap<String, Option<TokenInfo>> = self
            .http
            .post(self.site.join(TOKEN_TEST_PATH).expect("constant path"))
            .header(CONTENT_TYPE, "text/plain")
            .body(token.expose_secret().to_owned())
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(tested.into_values().collect())
    }
}
