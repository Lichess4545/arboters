use kameo::actor::Recipient;
use nutype::nutype;

use super::target::{ChannelName, ChatUserId, TopicName};

#[nutype(derive(Debug, Clone, PartialEq, Eq, Deref, Deserialize))]
pub struct MessageText(String);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OverheardMessage {
    pub sender: ChatUserId,
    pub channel: ChannelName,
    pub topic: TopicName,
    pub text: MessageText,
}

#[derive(Debug)]
pub struct Overhear(pub Recipient<OverheardMessage>);
