use sqlx::PgPool;

use crate::chat::ChatUserId;
use crate::lichess;

use super::{Accounts, AccountsError};

#[derive(Clone, Debug)]
pub struct PostgresAccounts {
    pool: PgPool,
}

impl PostgresAccounts {
    #[must_use]
    pub const fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn stored_user_id(user_id: ChatUserId) -> Result<i64, sqlx::Error> {
    i64::try_from(user_id.0).map_err(|error| sqlx::Error::Encode(Box::new(error)))
}

impl Accounts for PostgresAccounts {
    async fn link(
        &self,
        chat_user: ChatUserId,
        lichess_user: &lichess::UserId,
    ) -> Result<(), AccountsError> {
        sqlx::query!(
            r#"
            INSERT INTO linked_accounts (chat_user_id, lichess_user_id)
            VALUES ($1, $2)
            ON CONFLICT (chat_user_id)
            DO UPDATE SET lichess_user_id = EXCLUDED.lichess_user_id, linked_at = now()
            "#,
            stored_user_id(chat_user)?,
            lichess_user.as_str(),
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
