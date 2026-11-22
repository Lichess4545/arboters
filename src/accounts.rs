use kameo::error::SendError;

use crate::chat::{ChatUserId, WatchJoins};
use crate::lichess;

mod linker;
mod postgres;

pub use linker::Linker;
pub use postgres::PostgresAccounts;

#[derive(Debug, thiserror::Error)]
#[error("accounts query failed")]
pub struct AccountsError(#[from] sqlx::Error);

pub trait Accounts: Clone + Send + Sync + 'static {
    fn link(
        &self,
        chat_user: ChatUserId,
        lichess_user: &lichess::UserId,
    ) -> impl Future<Output = Result<(), AccountsError>> + Send;
}

#[derive(Debug, thiserror::Error)]
#[error("the linker could not watch the inbox for users who join")]
pub struct InboxUnavailable(#[from] SendError<WatchJoins>);
