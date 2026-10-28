use super::*;

fn is_valid(raw: &str) -> bool {
    UserId::try_new(raw).is_ok()
}

#[test]
fn whitespace_around_a_user_id_is_trimmed() {
    assert_eq!(UserId::try_new(" alice\n").unwrap().as_str(), "alice");
}

#[test]
fn user_ids_must_be_two_to_thirty_characters_long() {
    assert!(!is_valid("a"));
    assert!(is_valid("ab"));
    assert!(is_valid(&"a".repeat(30)));
    assert!(!is_valid(&"a".repeat(31)));
}

#[test]
fn user_ids_may_hold_only_ascii_letters_digits_underscores_and_hyphens() {
    assert!(is_valid("Mr_Scribbles-4545"));
    assert!(!is_valid("mr scribbles"));
    assert!(!is_valid("mr.scribbles"));
    assert!(!is_valid("josé"));
}

#[test]
fn invalid_user_ids_are_rejected_when_deserialized() {
    assert!(serde_json::from_str::<UserId>("\"mr scribbles\"").is_err());
}
