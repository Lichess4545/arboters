use sqlx::PgPool;

use crate::chat::MessageText;

use super::model::{NewResponse, Response, ResponseId};
use super::phrase::{Matching, Phrase};
use super::{Responses, ResponsesError};

#[derive(Clone, Debug)]
pub struct PostgresResponses {
    pool: PgPool,
}

impl PostgresResponses {
    #[must_use]
    pub const fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "response_matching", rename_all = "snake_case")]
enum StoredMatching {
    AnyCase,
    ExactCase,
}

impl From<Matching> for StoredMatching {
    fn from(matching: Matching) -> Self {
        match matching {
            Matching::AnyCase => Self::AnyCase,
            Matching::ExactCase => Self::ExactCase,
        }
    }
}

impl From<StoredMatching> for Matching {
    fn from(matching: StoredMatching) -> Self {
        match matching {
            StoredMatching::AnyCase => Self::AnyCase,
            StoredMatching::ExactCase => Self::ExactCase,
        }
    }
}

fn stored_id(id: u64) -> Result<i64, sqlx::Error> {
    i64::try_from(id).map_err(|error| sqlx::Error::Encode(Box::new(error)))
}

impl Responses for PostgresResponses {
    async fn add(&self, response: NewResponse) -> Result<ResponseId, ResponsesError> {
        let id = sqlx::query_scalar!(
            r#"
            INSERT INTO responses (phrase, matching, markdown, source_message_id, added_by_user_id)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (lower(phrase), source_message_id)
            DO UPDATE SET
                phrase = EXCLUDED.phrase,
                matching = EXCLUDED.matching,
                markdown = EXCLUDED.markdown,
                added_by_user_id = EXCLUDED.added_by_user_id
            RETURNING id
            "#,
            response.phrase.as_str(),
            StoredMatching::from(response.matching) as StoredMatching,
            response.markdown.as_str(),
            stored_id(response.source.0)?,
            stored_id(response.added_by.0)?,
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(ResponseId(id))
    }

    async fn list(&self, phrase: Option<&Phrase>) -> Result<Vec<Response>, ResponsesError> {
        let rows = sqlx::query!(
            r#"
            SELECT id, phrase, matching AS "matching: StoredMatching", markdown
            FROM responses
            WHERE $1::TEXT IS NULL OR lower(phrase) = lower($1)
            ORDER BY lower(phrase), id
            "#,
            phrase.map(|phrase| phrase.as_str()),
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(Response {
                    id: ResponseId(row.id),
                    phrase: Phrase::try_new(row.phrase)
                        .map_err(|error| sqlx::Error::Decode(Box::new(error)))?,
                    matching: row.matching.into(),
                    markdown: MessageText::new(row.markdown),
                })
            })
            .collect()
    }

    async fn remove(&self, id: ResponseId) -> Result<bool, ResponsesError> {
        let deleted = sqlx::query!("DELETE FROM responses WHERE id = $1", id.0)
            .execute(&self.pool)
            .await?;
        Ok(deleted.rows_affected() > 0)
    }
}

#[cfg(test)]
mod tests;
