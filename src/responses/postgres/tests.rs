use sqlx::PgPool;

use super::*;
use crate::chat::{ChatUserId, MessageId};

const MODERATOR: ChatUserId = ChatUserId(11);
const OTHER_MODERATOR: ChatUserId = ChatUserId(12);

fn phrase(text: &str) -> Phrase {
    Phrase::try_new(text).unwrap()
}

fn new_response(phrase_text: &str, source: u64, markdown: &str) -> NewResponse {
    NewResponse {
        phrase: phrase(phrase_text),
        matching: Matching::AnyCase,
        markdown: MessageText::new(markdown.to_owned()),
        source: MessageId(source),
        added_by: MODERATOR,
    }
}

fn exact_response(phrase_text: &str, source: u64, markdown: &str) -> NewResponse {
    NewResponse {
        matching: Matching::ExactCase,
        ..new_response(phrase_text, source, markdown)
    }
}

fn summary(responses: &[Response]) -> Vec<(&str, Matching, &str)> {
    responses
        .iter()
        .map(|response| {
            (
                response.phrase.as_str(),
                response.matching,
                response.markdown.as_str(),
            )
        })
        .collect()
}

#[sqlx::test]
async fn responses_are_listed_by_phrase_in_any_case_then_in_the_order_they_were_added(
    pool: PgPool,
) {
    let responses = PostgresResponses::new(pool);
    responses
        .add(new_response("rules", 1, "Read the rules."))
        .await
        .unwrap();
    responses
        .add(new_response("gg", 2, "Good game!"))
        .await
        .unwrap();
    responses
        .add(exact_response("GG", 3, "GG WP!"))
        .await
        .unwrap();
    responses
        .add(new_response("gg", 4, "Well played!"))
        .await
        .unwrap();

    let listed = responses.list(None).await.unwrap();

    assert_eq!(
        summary(&listed),
        [
            ("gg", Matching::AnyCase, "Good game!"),
            ("GG", Matching::ExactCase, "GG WP!"),
            ("gg", Matching::AnyCase, "Well played!"),
            ("rules", Matching::AnyCase, "Read the rules."),
        ]
    );
}

#[sqlx::test]
async fn listing_a_phrase_lists_its_responses_in_any_case(pool: PgPool) {
    let responses = PostgresResponses::new(pool);
    responses
        .add(new_response("rules", 1, "Read the rules."))
        .await
        .unwrap();
    responses
        .add(new_response("gg", 2, "Good game!"))
        .await
        .unwrap();
    responses
        .add(exact_response("GG", 3, "GG WP!"))
        .await
        .unwrap();

    let listed = responses.list(Some(&phrase("Gg"))).await.unwrap();

    assert_eq!(
        summary(&listed),
        [
            ("gg", Matching::AnyCase, "Good game!"),
            ("GG", Matching::ExactCase, "GG WP!"),
        ]
    );
}

#[sqlx::test]
async fn adding_the_same_message_to_a_phrase_again_refreshes_its_copy(pool: PgPool) {
    let responses = PostgresResponses::new(pool);
    let first = responses
        .add(new_response("gg", 2, "Good game!"))
        .await
        .unwrap();

    let again = responses
        .add(NewResponse {
            added_by: OTHER_MODERATOR,
            ..new_response("gg", 2, "Good game, everyone!")
        })
        .await
        .unwrap();

    assert_eq!(again, first);
    assert_eq!(
        summary(&responses.list(None).await.unwrap()),
        [("gg", Matching::AnyCase, "Good game, everyone!")]
    );
}

#[sqlx::test]
async fn adding_the_same_message_to_the_phrase_in_another_case_takes_the_new_case_and_matching(
    pool: PgPool,
) {
    let responses = PostgresResponses::new(pool);
    let first = responses
        .add(new_response("gg", 2, "Good game!"))
        .await
        .unwrap();

    let again = responses
        .add(exact_response("GG", 2, "Good game!"))
        .await
        .unwrap();

    assert_eq!(again, first);
    assert_eq!(
        summary(&responses.list(None).await.unwrap()),
        [("GG", Matching::ExactCase, "Good game!")]
    );
}

#[sqlx::test]
async fn the_same_message_can_answer_different_phrases(pool: PgPool) {
    let responses = PostgresResponses::new(pool);

    let gg = responses
        .add(new_response("gg", 2, "Good game!"))
        .await
        .unwrap();
    let wp = responses
        .add(new_response("wp", 2, "Good game!"))
        .await
        .unwrap();

    assert_ne!(gg, wp);
}

#[sqlx::test]
async fn removing_a_response_reports_whether_it_existed(pool: PgPool) {
    let responses = PostgresResponses::new(pool);
    let id = responses
        .add(new_response("gg", 2, "Good game!"))
        .await
        .unwrap();

    assert!(responses.remove(id).await.unwrap());
    assert!(!responses.remove(id).await.unwrap());
    assert_eq!(responses.list(None).await.unwrap(), []);
}
