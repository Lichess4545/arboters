use std::fmt;

use crate::games::{GameEventKind, LichessUsername};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SubscriptionId(pub i64);

impl fmt::Display for SubscriptionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChatUserId(pub u64);

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChannelName(String);

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TopicName(String);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("chat names must not be blank")]
pub struct BlankChatName;

impl ChannelName {
    pub fn parse(raw: &str) -> Result<Self, BlankChatName> {
        non_blank(raw).map(Self)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TopicName {
    pub fn parse(raw: &str) -> Result<Self, BlankChatName> {
        non_blank(raw).map(Self)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn non_blank(raw: &str) -> Result<String, BlankChatName> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        Err(BlankChatName)
    } else {
        Ok(trimmed.to_owned())
    }
}

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewSubscription {
    pub requester: ChatUserId,
    pub source: LichessUsername,
    pub event: GameEventKind,
    pub target: NotificationTarget,
}
