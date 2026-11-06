use super::*;

#[test]
fn a_round_has_ended_by_the_moment_it_ends() {
    let ends = OffsetDateTime::UNIX_EPOCH;
    let round = Round {
        id: RoundId::new(812),
        starts: None,
        ends: Some(ends),
    };

    assert!(round.has_ended_by(ends));
}
