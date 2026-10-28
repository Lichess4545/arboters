use crate::subscriptions::NotificationTarget;

#[derive(Debug, thiserror::Error)]
#[error("failed to deliver notification to {target:?}")]
pub struct DeliveryError {
    pub target: NotificationTarget,
    #[source]
    pub cause: Box<dyn std::error::Error + Send + Sync>,
}

pub trait Messenger: Send + Sync + 'static {
    fn send(
        &self,
        target: &NotificationTarget,
        content: &str,
    ) -> impl Future<Output = Result<(), DeliveryError>> + Send;
}
