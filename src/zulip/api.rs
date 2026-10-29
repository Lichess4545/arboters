use std::time::Duration;

use nutype::nutype;
use reqwest::StatusCode;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use url::Url;

use crate::chat::{ChannelName, ChatUserId, Role, TopicName};

use super::settings::{BotEmail, ZulipSettings};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const LONG_POLL_TIMEOUT: Duration = Duration::from_secs(90);

#[derive(Clone, Debug)]
pub struct Api {
    http: reqwest::Client,
    site: Url,
    email: BotEmail,
    key: SecretString,
}

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("zulip endpoint url is invalid")]
    Url(#[from] url::ParseError),
    #[error("zulip request failed")]
    Request(#[from] reqwest::Error),
    #[error("zulip answered {status} with a body that is not a zulip response")]
    Unreadable {
        status: StatusCode,
        #[source]
        cause: serde_json::Error,
    },
    #[error("zulip no longer has the event queue")]
    BadEventQueue,
    #[error("zulip rejected the request ({status}): {reason}")]
    Rejected { status: StatusCode, reason: String },
}

#[nutype(
    sanitize(trim),
    validate(not_empty),
    derive(Debug, Clone, PartialEq, Eq, Deref, Deserialize)
)]
pub struct FullName(String);

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Bot {
    #[serde(rename = "user_id")]
    pub id: ChatUserId,
    pub full_name: FullName,
}

#[nutype(
    validate(not_empty),
    derive(Debug, Clone, PartialEq, Eq, Deref, Deserialize, Display)
)]
pub struct QueueId(String);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
pub struct EventId(i64);

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Queue {
    #[serde(rename = "queue_id")]
    pub id: QueueId,
    pub last_event_id: EventId,
}

#[derive(Debug)]
pub struct Batch {
    pub last_event_id: Option<EventId>,
    pub messages: Vec<Delivery>,
}

#[derive(Debug, Deserialize)]
pub struct Delivery {
    pub message: Message,
    pub flags: Vec<Flag>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Flag {
    Mentioned,
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
pub struct Message {
    pub sender_id: ChatUserId,
    pub content: String,
    #[serde(flatten)]
    pub place: Place,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Place {
    Stream {
        display_recipient: ChannelName,
        subject: TopicName,
    },
    Private {
        display_recipient: Vec<Recipient>,
    },
}

#[derive(Debug, Deserialize)]
pub struct Recipient {
    pub id: ChatUserId,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Outcome {
    Success,
    Error,
}

#[derive(Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum ErrorCode {
    BadEventQueueId,
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
struct Envelope {
    result: Outcome,
    code: Option<ErrorCode>,
    #[serde(default)]
    msg: String,
}

#[derive(Deserialize)]
struct Events {
    events: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
struct Numbered {
    id: EventId,
    #[serde(rename = "type")]
    kind: EventKind,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum EventKind {
    Message,
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
struct UserEnvelope {
    user: UserRole,
}

#[derive(Deserialize)]
struct UserRole {
    role: u16,
}

const fn role_from(value: u16) -> Role {
    match value {
        100 => Role::Owner,
        200 => Role::Administrator,
        300 => Role::Moderator,
        400 => Role::Member,
        _ => Role::Guest,
    }
}

impl Api {
    #[must_use]
    pub fn new(http: reqwest::Client, settings: &ZulipSettings) -> Self {
        Self {
            http,
            site: settings.site.clone(),
            email: settings.email.clone(),
            key: settings.api_key.clone(),
        }
    }

    fn endpoint(&self, path: &str) -> Result<Url, ApiError> {
        Ok(self.site.join("api/v1/")?.join(path)?)
    }

    fn get(&self, path: &str) -> Result<reqwest::RequestBuilder, ApiError> {
        Ok(self
            .http
            .get(self.endpoint(path)?)
            .basic_auth(self.email.as_str(), Some(self.key.expose_secret()))
            .timeout(REQUEST_TIMEOUT))
    }

    pub async fn own_user(&self) -> Result<Bot, ApiError> {
        read(self.get("users/me")?.send().await?).await
    }

    pub async fn role_of(&self, user_id: ChatUserId) -> Result<Role, ApiError> {
        let found: UserEnvelope =
            read(self.get(&format!("users/{}", user_id.0))?.send().await?).await?;
        Ok(role_from(found.user.role))
    }

    pub async fn register(&self) -> Result<Queue, ApiError> {
        let response = self
            .http
            .post(self.endpoint("register")?)
            .basic_auth(self.email.as_str(), Some(self.key.expose_secret()))
            .timeout(REQUEST_TIMEOUT)
            .form(&[
                ("event_types", r#"["message"]"#),
                ("apply_markdown", "false"),
            ])
            .send()
            .await?;
        read(response).await
    }

    pub async fn events(&self, queue: &Queue) -> Result<Batch, ApiError> {
        let response = self
            .get("events")?
            .query(&[
                ("queue_id", queue.id.to_string()),
                ("last_event_id", queue.last_event_id.0.to_string()),
            ])
            .timeout(LONG_POLL_TIMEOUT)
            .send()
            .await?;
        let Events { events } = read(response).await?;
        let mut batch = Batch {
            last_event_id: None,
            messages: Vec::new(),
        };
        for event in events {
            let Ok(numbered) = Numbered::deserialize(&event) else {
                tracing::warn!(%event, "zulip event without an id or type ignored");
                continue;
            };
            batch.last_event_id = batch.last_event_id.max(Some(numbered.id));
            if !matches!(numbered.kind, EventKind::Message) {
                continue;
            }
            match Delivery::deserialize(&event) {
                Ok(delivery) => batch.messages.push(delivery),
                Err(error) => {
                    tracing::warn!(%error, event_id = numbered.id.0, "unreadable zulip message event ignored");
                }
            }
        }
        Ok(batch)
    }
}

impl Queue {
    pub fn acknowledge(&mut self, batch: &Batch) {
        if let Some(last_event_id) = batch.last_event_id {
            self.last_event_id = self.last_event_id.max(last_event_id);
        }
    }
}

async fn read<T: DeserializeOwned>(response: reqwest::Response) -> Result<T, ApiError> {
    let status = response.status();
    let body = response.bytes().await?;
    let unreadable = |cause| ApiError::Unreadable { status, cause };
    let envelope: Envelope = serde_json::from_slice(&body).map_err(unreadable)?;
    match (envelope.result, envelope.code) {
        (Outcome::Success, _) => serde_json::from_slice(&body).map_err(unreadable),
        (Outcome::Error, Some(ErrorCode::BadEventQueueId)) => Err(ApiError::BadEventQueue),
        (Outcome::Error, _) => Err(ApiError::Rejected {
            status,
            reason: envelope.msg,
        }),
    }
}

#[cfg(test)]
mod tests;
