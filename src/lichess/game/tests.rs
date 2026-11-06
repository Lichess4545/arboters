use super::*;

#[test]
fn a_status_lichess_always_gives_a_winner_is_impossible_without_one() {
    for status in [
        StatusName::Mate,
        StatusName::Resign,
        StatusName::Cheat,
        StatusName::NoStart,
        StatusName::VariantEnd,
    ] {
        assert!(GameStatus::new(status, None).is_err(), "{status:?}");
    }
}

#[test]
fn a_status_lichess_never_gives_a_winner_is_impossible_with_one() {
    for status in [
        StatusName::Created,
        StatusName::Started,
        StatusName::Aborted,
        StatusName::Stalemate,
        StatusName::Draw,
        StatusName::InsufficientMaterialClaim,
    ] {
        assert!(
            GameStatus::new(status, Some(Color::White)).is_err(),
            "{status:?}"
        );
    }
}

#[test]
fn a_timeout_a_flag_or_an_unknown_finish_may_have_a_winner_or_not() {
    for status in [
        StatusName::Timeout,
        StatusName::Outoftime,
        StatusName::UnknownFinish,
    ] {
        for winner in [None, Some(Color::Black)] {
            assert!(
                GameStatus::new(status, winner).is_ok(),
                "{status:?} {winner:?}"
            );
        }
    }
}
