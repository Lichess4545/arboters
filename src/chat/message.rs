use kameo::actor::Recipient;
use nutype::nutype;
use serde::Deserialize;

use super::conversation::{ChannelName, ChatUserId, TopicName};

#[nutype(derive(Debug, Clone, PartialEq, Eq, Deref, Deserialize))]
pub struct MessageText(String);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
pub struct MessageId(pub u64);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OverheardMessage {
    pub sender: ChatUserId,
    pub channel: ChannelName,
    pub topic: TopicName,
    pub text: MessageText,
}

#[derive(Debug)]
pub struct Overhear(pub Recipient<OverheardMessage>);
