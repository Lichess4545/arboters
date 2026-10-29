use secrecy::ExposeSecret;
use zulip_api::api::{ApiKey, User};
use zulip_api::client::Client;
use zulip_api::response::{ApiResult, ErrorCode, Response as _};
use zulip_api::send_message;
use zulip_api::{MessageTo, MessageType};

use crate::chat::{DeliveryError, Messenger, Notification, NotificationTarget};

use super::settings::ZulipSettings;

#[derive(Clone, Debug)]
pub struct ZulipMessenger {
    client: Client,
}

#[derive(Debug, thiserror::Error)]
pub enum ZulipSetupError {
    #[error("zulip site `{site}` is not a usable uri")]
    InvalidSite {
        site: url::Url,
        #[source]
        cause: Box<dyn std::error::Error + Send + Sync>,
    },
    #[error("could not build the zulip client")]
    Client(#[from] zulip_api::Error),
}

#[derive(Debug, thiserror::Error)]
#[error("zulip rejected the message ({code:?}): {reason}")]
pub struct ZulipRejection {
    pub code: Option<ErrorCode>,
    pub reason: String,
}

impl ZulipMessenger {
    pub fn new(settings: &ZulipSettings) -> Result<Self, ZulipSetupError> {
        let site =
            settings
                .site
                .as_str()
                .parse()
                .map_err(|cause| ZulipSetupError::InvalidSite {
                    site: settings.site.clone(),
                    cause: Box::new(cause),
                })?;
        let client = Client::new(
            site,
            User::from(settings.email.as_str()),
            ApiKey::from(settings.api_key.expose_secret()),
        )?;
        Ok(Self { client })
    }
}

fn request_for(target: &NotificationTarget, notification: &Notification) -> send_message::Request {
    let request = send_message::Request::new().with_content(notification.markdown().to_owned());
    match target {
        NotificationTarget::DirectMessage { user_id } => request
            .with_msg_type(MessageType::Direct)
            .with_to(MessageTo::ChannelName(format!("[{}]", user_id.0))),
        NotificationTarget::Channel { channel, topic } => request
            .with_msg_type(MessageType::Channel)
            .with_to(MessageTo::channel_name(channel.as_str()))
            .with_topic(topic.as_str()),
    }
}

impl Messenger for ZulipMessenger {
    async fn send(
        &self,
        target: &NotificationTarget,
        notification: &Notification,
    ) -> Result<(), DeliveryError> {
        let failed = |cause: Box<dyn std::error::Error + Send + Sync>| DeliveryError {
            target: target.clone(),
            cause,
        };
        let response = self
            .client
            .send_message(&request_for(target, notification))
            .await
            .map_err(|error| failed(Box::new(error)))?;
        match response.result() {
            ApiResult::Success => Ok(()),
            ApiResult::Error => Err(failed(Box::new(ZulipRejection {
                code: response.code(),
                reason: response.msg().to_owned(),
            }))),
        }
    }
}

#[cfg(test)]
mod tests;
