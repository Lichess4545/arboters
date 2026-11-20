use crate::chat::ChatUserId;
use crate::lichess;

mod postgres;

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
