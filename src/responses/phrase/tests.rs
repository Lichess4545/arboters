use super::*;

fn phrase(text: &str) -> Phrase {
    Phrase::try_new(text).unwrap()
}

fn appears(phrase_text: &str, text: &str) -> bool {
    phrase(phrase_text).appears_in(text, Matching::AnyCase)
}

fn appears_exactly(phrase_text: &str, text: &str) -> bool {
    phrase(phrase_text).appears_in(text, Matching::ExactCase)
}

#[test]
fn a_phrase_keeps_its_case_and_collapses_its_whitespace() {
    assert_eq!(phrase("  Good \n Game ").as_str(), "Good Game");
}

#[test]
fn a_phrase_matches_whole_words_only() {
    assert!(appears("gg", "gg"));
    assert!(appears("gg", "well played, gg"));
    assert!(appears("gg", "gg everyone"));
    assert!(!appears("gg", "eggs"));
    assert!(!appears("gg", "ggs"));
    assert!(!appears("gg", "egg"));
}

#[test]
fn a_phrase_matches_in_any_case() {
    assert!(appears("Good Game", "GOOD GAME"));
    assert!(appears("good game", "Good Game"));
}

#[test]
fn an_exact_phrase_matches_only_in_its_own_case() {
    assert!(appears_exactly("GG", "That was close. GG!"));
    assert!(!appears_exactly("GG", "gg"));
    assert!(!appears_exactly("GG", "Gg"));
    assert!(!appears_exactly("Good Game", "good game"));
    assert!(appears_exactly("Good Game", "a Good   Game"));
}

#[test]
fn an_exact_phrase_still_matches_whole_words_only() {
    assert!(!appears_exactly("GG", "EGGS"));
    assert!(!appears_exactly("GG", "GGs"));
    assert!(appears_exactly("GG", "(GG)"));
    assert!(!appears_exactly("Good Game", "GoodGame"));
}

#[test]
fn punctuation_around_a_phrase_still_counts_as_a_word_boundary() {
    assert!(appears("gg", "gg!"));
    assert!(appears("gg", "(gg)"));
    assert!(appears("gg", "**gg**"));
    assert!(appears("gg", "that's it... gg."));
}

#[test]
fn the_words_of_a_phrase_may_be_separated_by_any_whitespace() {
    assert!(appears("good game", "good   game"));
    assert!(appears("good game", "good\ngame"));
    assert!(!appears("good game", "goodgame"));
    assert!(!appears("good game", "good, game"));
}

#[test]
fn a_phrase_that_ends_in_punctuation_needs_that_punctuation() {
    assert!(appears("rules?", "what are the rules?"));
    assert!(!appears("rules?", "what are the rules"));
}

#[test]
fn a_phrase_may_be_found_after_an_earlier_partial_match() {
    assert!(appears("good game", "good good game"));
}

#[test]
fn a_blank_phrase_is_not_a_phrase() {
    assert!(Phrase::try_new("   ").is_err());
}
