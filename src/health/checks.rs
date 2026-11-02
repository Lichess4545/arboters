use std::collections::HashMap;

use reqwest::header::CONTENT_TYPE;
use reqwest::{Response, StatusCode};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use sqlx::PgPool;
use sqlx::postgres::{PgDatabaseError, PgSeverity};
use url::Url;

use crate::game_monitor::LichessSettings;
use crate::heltour::Heltour;
use crate::lichess;
use crate::zulip::{BotEmail, ZulipSettings};

use super::TokenOwner;

const LICHESS_TOKEN_TEST_PATH: &str = "api/token/test";
const ZULIP_OWN_USER_PATH: &str = "api/v1/users/me";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Retry,
    Report,
    Exit,
}

#[derive(Debug)]
pub struct Failure {
    pub outcome: Outcome,
    pub error: String,
}

impl Failure {
    fn new(outcome: Outcome, error: impl Into<anyhow::Error>) -> Self {
        Self {
            outcome,
            error: format!("{:#}", error.into()),
        }
    }

    pub fn retry(error: impl Into<anyhow::Error>) -> Self {
        Self::new(Outcome::Retry, error)
    }
}

#[derive(Debug)]
pub struct Token {
    url: Url,
    secret: SecretString,
}

impl Token {
    pub fn lichess(lichess: &LichessSettings) -> Result<Option<Self>, url::ParseError> {
        let Some(secret) = lichess.token.clone() else {
            return Ok(None);
        };
        Ok(Some(Self {
            url: lichess.base_url.join(LICHESS_TOKEN_TEST_PATH)?,
            secret,
        }))
    }
}

#[derive(Debug)]
pub struct ZulipBot {
    url: Url,
    email: BotEmail,
    api_key: SecretString,
}

impl ZulipBot {
    pub fn new(zulip: &ZulipSettings) -> Result<Self, url::ParseError> {
        Ok(Self {
            url: zulip.site.join(ZULIP_OWN_USER_PATH)?,
            email: zulip.email.clone(),
            api_key: zulip.api_key.clone(),
        })
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Owner {
    user_id: lichess::UserId,
}

pub async fn read_our_tables(pool: &PgPool) -> Result<(), Failure> {
    sqlx::query!("SELECT FROM subscriptions, responses LIMIT 0")
        .execute(pool)
        .await
        .map(drop)
        .map_err(|error| Failure::new(outcome_of_database(&error), error))
}

pub async fn test_heltour_token(heltour: &Heltour) -> Result<(), Failure> {
    heltour.test_token().await.map_err(failed_request)
}

pub async fn lichess_token_owner(
    http: &reqwest::Client,
    token: &Token,
) -> Result<TokenOwner, Failure> {
    let response = http
        .post(token.url.clone())
        .header(CONTENT_TYPE, "text/plain")
        .body(token.secret.expose_secret().to_owned())
        .send()
        .await
        .map_err(Failure::retry)?;
    let owners: HashMap<String, Option<Owner>> =
        accepted(response)?.json().await.map_err(Failure::retry)?;
    owners
        .into_values()
        .flatten()
        .next()
        .map(|owner| TokenOwner(owner.user_id))
        .ok_or_else(|| {
            Failure::new(
                Outcome::Report,
                anyhow::anyhow!("lichess reports the token invalid"),
            )
        })
}

pub async fn find_zulip_bot(http: &reqwest::Client, bot: &ZulipBot) -> Result<(), Failure> {
    http.get(bot.url.clone())
        .basic_auth(bot.email.as_str(), Some(bot.api_key.expose_secret()))
        .send()
        .await
        .and_then(Response::error_for_status)
        .map(drop)
        .map_err(Failure::retry)
}

fn accepted(response: Response) -> Result<Response, Failure> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    Err(refused(status))
}

fn failed_request(error: reqwest::Error) -> Failure {
    error
        .status()
        .map_or_else(|| Failure::retry(error), refused)
}

fn refused(status: StatusCode) -> Failure {
    let outcome = match status {
        StatusCode::REQUEST_TIMEOUT | StatusCode::TOO_MANY_REQUESTS => Outcome::Retry,
        status if status.is_client_error() => Outcome::Report,
        _ => Outcome::Retry,
    };
    Failure::new(outcome, anyhow::anyhow!("answered {status}"))
}

// https://www.postgresql.org/docs/current/errcodes-appendix.html#ERRCODES-TABLE
const SQLSTATE_CLASS_LENGTH: usize = 2;
const CONNECTION_EXCEPTION: &str = "08";
const TRANSACTION_ROLLBACK: &str = "40";
const INSUFFICIENT_RESOURCES: &str = "53";
const OPERATOR_INTERVENTION: &str = "57";
const SYSTEM_ERROR: &str = "58";
const INVALID_AUTHORIZATION_SPECIFICATION: &str = "28";
const INVALID_CATALOG_NAME: &str = "3D";
const SYNTAX_ERROR_OR_ACCESS_RULE_VIOLATION: &str = "42";
const INTERNAL_ERROR: &str = "XX";

fn outcome_of_database(error: &sqlx::Error) -> Outcome {
    let sqlx::Error::Database(refusal) = error else {
        return match error {
            sqlx::Error::Io(_) | sqlx::Error::PoolTimedOut | sqlx::Error::Protocol(_) => {
                Outcome::Retry
            }
            _ => Outcome::Exit,
        };
    };
    let class = refusal
        .code()
        .and_then(|code| code.get(..SQLSTATE_CLASS_LENGTH).map(str::to_owned));
    match class.as_deref() {
        Some(
            CONNECTION_EXCEPTION
            | TRANSACTION_ROLLBACK
            | INSUFFICIENT_RESOURCES
            | OPERATOR_INTERVENTION
            | SYSTEM_ERROR,
        ) => Outcome::Retry,
        Some(
            INVALID_AUTHORIZATION_SPECIFICATION
            | INVALID_CATALOG_NAME
            | SYNTAX_ERROR_OR_ACCESS_RULE_VIOLATION
            | INTERNAL_ERROR,
        ) => Outcome::Exit,
        _ => match refusal
            .try_downcast_ref::<PgDatabaseError>()
            .map(PgDatabaseError::severity)
        {
            Some(PgSeverity::Fatal | PgSeverity::Panic) => Outcome::Retry,
            _ => Outcome::Exit,
        },
    }
}
