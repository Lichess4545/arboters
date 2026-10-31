use kameo::Reply;
use kameo::actor::ReplyRecipient;
use nutype::nutype;

use super::conversation::{ChatUserId, Conversation};
use super::message::MessageId;
use super::outgoing::OutgoingMessage;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Owner,
    Administrator,
    Moderator,
    Member,
    Guest,
}

impl Role {
    /// ```
    /// use arboters::chat::Role;
    ///
    /// assert!(Role::Owner.moderates());
    /// assert!(Role::Administrator.moderates());
    /// assert!(Role::Moderator.moderates());
    /// assert!(!Role::Member.moderates());
    /// assert!(!Role::Guest.moderates());
    /// ```
    #[must_use]
    pub const fn moderates(self) -> bool {
        matches!(self, Self::Owner | Self::Administrator | Self::Moderator)
    }
}

/// ```
/// use arboters::chat::CommandText;
///
/// assert_eq!(CommandText::new("  subscription list \n").as_str(), "subscription list");
/// ```
#[nutype(sanitize(trim), derive(Debug, Clone, PartialEq, Eq, Deref))]
pub struct CommandText(String);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Revision {
    Original,
    Edited,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IncomingMessage {
    pub id: MessageId,
    pub revision: Revision,
    pub sender: ChatUserId,
    pub role: Role,
    pub reply_to: Conversation,
    pub text: CommandText,
}

#[derive(Clone, Debug, PartialEq, Eq, Reply)]
pub enum Handling {
    Answered(OutgoingMessage),
    Unrecognized,
}

#[derive(Debug)]
pub struct Listen(pub ReplyRecipient<IncomingMessage, Handling>);
