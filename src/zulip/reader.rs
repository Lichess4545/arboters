use reqwest::StatusCode;

use crate::chat::{MessageId, MessageText};
use crate::responses::{MessageReader, ReadError};

use super::Config;
use super::api::{Api, ApiError};

#[derive(Clone, Debug)]
pub struct ZulipMessageReader {
    api: Api,
}

impl ZulipMessageReader {
    #[must_use]
    pub fn new(http: reqwest::Client, config: &Config) -> Self {
        Self {
            api: Api::new(http, config),
        }
    }
}

impl MessageReader for ZulipMessageReader {
    async fn read(&self, id: MessageId) -> Result<MessageText, ReadError> {
        self.api.markdown_of(id).await.map_err(|error| match error {
            ApiError::Rejected {
                status: StatusCode::BAD_REQUEST,
                ..
            } => ReadError::Unreadable,
            error => ReadError::Failed(Box::new(error)),
        })
    }
}

#[cfg(test)]
mod tests;
