use sqlx::PgPool;

use super::*;
use crate::testing::{linked_account, user_id};

const ALICE: ChatUserId = ChatUserId(23);

#[sqlx::test]
async fn a_linked_user_has_their_lichess_account(pool: PgPool) {
    let accounts = PostgresAccounts::new(pool.clone());

    accounts.link(ALICE, &user_id("alice")).await.unwrap();

    assert_eq!(linked_account(&pool, ALICE).await, ["alice"]);
}

#[sqlx::test]
async fn linking_a_user_again_replaces_their_lichess_account(pool: PgPool) {
    let accounts = PostgresAccounts::new(pool.clone());
    accounts.link(ALICE, &user_id("alice")).await.unwrap();

    accounts.link(ALICE, &user_id("alice_alt")).await.unwrap();

    assert_eq!(linked_account(&pool, ALICE).await, ["alice_alt"]);
}
