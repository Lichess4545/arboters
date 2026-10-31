use super::conversation::Conversation;
use super::outgoing::OutgoingMessage;

#[derive(Debug, thiserror::Error)]
#[error("failed to deliver message to {conversation:?}")]
pub struct DeliveryError {
    pub conversation: Conversation,
    #[source]
    pub cause: Box<dyn std::error::Error + Send + Sync>,
}

pub trait Messenger: Send + Sync + 'static {
    fn send(
        &self,
        conversation: &Conversation,
        message: &OutgoingMessage,
    ) -> impl Future<Output = Result<(), DeliveryError>> + Send;
}
