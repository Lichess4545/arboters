use std::fmt;

use nutype::nutype;

use crate::game_monitor::GameEventKind;
use crate::heltour::{league, team};
use crate::lichess;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SubscriptionId(pub i64);

impl fmt::Display for SubscriptionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
    derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deref)
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

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SubscriptionSource {
    Player(lichess::UserId),
    Team(team::Name),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewSubscription {
    pub requester: ChatUserId,
    pub league: league::Tag,
    pub source: SubscriptionSource,
    pub event: GameEventKind,
    pub target: NotificationTarget,
}
