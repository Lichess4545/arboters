use super::conversation::Conversation;
use super::messenger::{DeliveryError, Messenger};
use super::outgoing::OutgoingMessage;

#[derive(Clone, Copy, Debug, Default)]
pub struct LoggingMessenger;

impl Messenger for LoggingMessenger {
    fn send(
        &self,
        conversation: &Conversation,
        message: &OutgoingMessage,
    ) -> impl Future<Output = Result<(), DeliveryError>> + Send {
        tracing::info!(
            ?conversation,
            message = message.markdown(),
            "message not delivered because no messenger is configured"
        );
        std::future::ready(Ok(()))
    }
}
