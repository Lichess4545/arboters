use super::*;

#[test]
fn short_words_must_be_exact_and_longer_words_allow_more_typos() {
    for (written, word, matching) in [
        ("in", "in", true),
        ("is", "in", false),
        ("tel", "tell", true),
        ("tl", "tell", false),
        ("strats", "starts", true),
        ("sat", "starts", false),
    ] {
        assert_eq!(matches(written, word), matching, "{written} as {word}");
    }
}

#[test]
fn the_closest_candidate_wins_and_an_equal_distance_is_a_tie() {
    let candidates = [("team4545", 1), ("team4546", 2), ("lonewolf", 3)];

    assert_eq!(closest("team4545", candidates), Closest::One(1));
    assert_eq!(closest("lonewlof", candidates), Closest::One(3));
    assert_eq!(closest("team454", candidates), Closest::Tie(vec![1, 2]));
    assert_eq!(closest("chess960", candidates), Closest::Nothing);
}
