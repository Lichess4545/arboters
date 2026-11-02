use reqwest::StatusCode;
use sqlx::PgPool;
use sqlx::postgres::{PgDatabaseError, PgSeverity};

use crate::heltour::Heltour;
use crate::lichess;
use crate::zulip;

use super::TokenOwner;

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

pub async fn lichess_token_owner(lichess: &lichess::Api) -> Result<TokenOwner, Failure> {
    lichess
        .token_owner()
        .await
        .map_err(failed_request)?
        .map(TokenOwner)
        .ok_or_else(|| {
            Failure::new(
                Outcome::Report,
                anyhow::anyhow!("lichess reports the token invalid"),
            )
        })
}

pub async fn find_zulip_bot(zulip: &zulip::Api) -> Result<(), Failure> {
    zulip.own_user().await.map(drop).map_err(|error| {
        let outcome = match &error {
            zulip::ApiError::Rejected { status, .. }
            | zulip::ApiError::Unreadable { status, .. } => outcome_of_status(*status),
            zulip::ApiError::Request(_) | zulip::ApiError::BadEventQueue => Outcome::Retry,
        };
        Failure::new(outcome, error)
    })
}

fn failed_request(error: reqwest::Error) -> Failure {
    error
        .status()
        .map_or_else(|| Failure::retry(error), refused)
}

fn refused(status: StatusCode) -> Failure {
    Failure::new(
        outcome_of_status(status),
        anyhow::anyhow!("answered {status}"),
    )
}

fn outcome_of_status(status: StatusCode) -> Outcome {
    match status {
        StatusCode::REQUEST_TIMEOUT | StatusCode::TOO_MANY_REQUESTS => Outcome::Retry,
        status if status.is_client_error() => Outcome::Report,
        _ => Outcome::Retry,
    }
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
