use crate::chat::{Conversation, DeliveryError, MessageId, Messenger, OutgoingMessage};

use super::api::{Api, ApiError};
use super::settings::ZulipSettings;

#[derive(Clone, Debug)]
pub struct ZulipMessenger {
    api: Api,
}

impl ZulipMessenger {
    #[must_use]
    pub fn new(http: reqwest::Client, settings: &ZulipSettings) -> Self {
        Self {
            api: Api::new(http, settings),
        }
    }

    pub async fn post(
        &self,
        conversation: &Conversation,
        message: &OutgoingMessage,
    ) -> Result<MessageId, DeliveryError> {
        self.api
            .send_message(conversation, message)
            .await
            .map_err(failure(conversation))
    }

    pub async fn edit(
        &self,
        conversation: &Conversation,
        id: MessageId,
        message: &OutgoingMessage,
    ) -> Result<(), DeliveryError> {
        self.api
            .edit_message(id, message)
            .await
            .map_err(failure(conversation))
    }
}

impl Messenger for ZulipMessenger {
    async fn send(
        &self,
        conversation: &Conversation,
        message: &OutgoingMessage,
    ) -> Result<(), DeliveryError> {
        self.post(conversation, message).await.map(|_| ())
    }
}

fn failure(conversation: &Conversation) -> impl Fn(ApiError) -> DeliveryError + '_ {
    |error| DeliveryError {
        conversation: conversation.clone(),
        cause: Box::new(error),
    }
}

#[cfg(test)]
mod tests;
