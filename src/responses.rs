use kameo::error::SendError;

use crate::chat::{Listen, MessageId, MessageText, Overhear};

mod commands;
mod cooldown;
mod link;
mod model;
mod phrase;
mod postgres;
mod responder;
mod settings;

pub use commands::ResponseCommands;
pub use model::{NewResponse, Response, ResponseId};
pub use phrase::{Matching, Phrase, PhraseError};
pub use postgres::PostgresResponses;
pub use responder::Responder;
pub use settings::ResponsesSettings;

#[derive(Debug, thiserror::Error)]
#[error("responses query failed")]
pub struct ResponsesError(#[from] sqlx::Error);

pub trait Responses: Clone + Send + Sync + 'static {
    fn add(
        &self,
        response: NewResponse,
    ) -> impl Future<Output = Result<ResponseId, ResponsesError>> + Send;

    fn list(
        &self,
        phrase: Option<&Phrase>,
    ) -> impl Future<Output = Result<Vec<Response>, ResponsesError>> + Send;

    fn remove(&self, id: ResponseId) -> impl Future<Output = Result<bool, ResponsesError>> + Send;
}

#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    #[error("the message does not exist or the bot cannot see it")]
    Unreadable,
    #[error("could not read the message")]
    Failed(#[source] Box<dyn std::error::Error + Send + Sync>),
}

pub trait MessageReader: Send + Sync + 'static {
    fn read(&self, id: MessageId) -> impl Future<Output = Result<MessageText, ReadError>> + Send;
}

#[derive(Debug, thiserror::Error)]
pub enum InboxUnavailable {
    #[error("response commands could not listen to the inbox")]
    Listen(#[from] SendError<Listen>),
    #[error("the responder could not overhear the inbox")]
    Overhear(#[from] SendError<Overhear>),
}
