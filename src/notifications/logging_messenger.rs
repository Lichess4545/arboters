use crate::subscriptions::NotificationTarget;

use super::messenger::{DeliveryError, Messenger};
use super::rendering::Notification;

#[derive(Clone, Copy, Debug, Default)]
pub struct LoggingMessenger;

impl Messenger for LoggingMessenger {
    fn send(
        &self,
        target: &NotificationTarget,
        notification: &Notification,
    ) -> impl Future<Output = Result<(), DeliveryError>> + Send {
        tracing::info!(
            ?target,
            notification = notification.markdown(),
            "notification not delivered because no messenger is configured"
        );
        std::future::ready(Ok(()))
    }
}
