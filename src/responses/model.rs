use std::fmt;

use crate::chat::{ChatUserId, MessageId, MessageText};

use super::phrase::{Matching, Phrase};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResponseId(pub i64);

impl fmt::Display for ResponseId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewResponse {
    pub phrase: Phrase,
    pub matching: Matching,
    pub markdown: MessageText,
    pub source: MessageId,
    pub added_by: ChatUserId,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Response {
    pub id: ResponseId,
    pub phrase: Phrase,
    pub matching: Matching,
    pub markdown: MessageText,
}
