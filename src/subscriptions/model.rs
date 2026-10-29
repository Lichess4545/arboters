use std::fmt;

use crate::chat::{ChatUserId, NotificationTarget};
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
