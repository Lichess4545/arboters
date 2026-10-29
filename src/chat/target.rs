use nutype::nutype;
use serde::Deserialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
pub struct ChatUserId(pub u64);

#[nutype(
    sanitize(trim),
    validate(not_empty),
    derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deref, Deserialize)
)]
pub struct ChannelName(String);

#[nutype(
    sanitize(trim),
    validate(not_empty),
    derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deref, Deserialize)
)]
pub struct TopicName(String);

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NotificationTarget {
    DirectMessage {
        user_id: ChatUserId,
    },
    Channel {
        channel: ChannelName,
        topic: TopicName,
    },
}
