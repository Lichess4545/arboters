use super::*;
use crate::game_monitor::GameEvent;
use crate::lichess::GameState;
use crate::testing::alice_vs_bob;

const fn finished(outcome: GameOutcome, ending: Ending) -> GamePhase {
    GamePhase::Finished { outcome, ending }
}

#[test]
fn statuses_and_winners_map_to_game_phases() {
    let white = Some(Color::White);
    let black = Some(Color::Black);
    let cases = [
        (GameStatus::Created, None, GamePhase::Created),
        (GameStatus::Started, None, GamePhase::Started),
        (
            GameStatus::Aborted,
            None,
            finished(GameOutcome::Aborted, Ending::Aborted),
        ),
        (
            GameStatus::Mate,
            white,
            finished(GameOutcome::WhiteWon, Ending::Checkmate),
        ),
        (
            GameStatus::Resign,
            black,
            finished(GameOutcome::BlackWon, Ending::Resignation),
        ),
        (
            GameStatus::Stalemate,
            None,
            finished(GameOutcome::Draw, Ending::Stalemate),
        ),
        (
            GameStatus::Timeout,
            white,
            finished(GameOutcome::WhiteWon, Ending::VictoryClaimed),
        ),
        (
            GameStatus::Draw,
            None,
            finished(GameOutcome::Draw, Ending::Draw),
        ),
        (
            GameStatus::Outoftime,
            None,
            finished(GameOutcome::Draw, Ending::OutOfTime),
        ),
        (
            GameStatus::Outoftime,
            black,
            finished(GameOutcome::BlackWon, Ending::OutOfTime),
        ),
        (
            GameStatus::Cheat,
            black,
            finished(GameOutcome::BlackWon, Ending::CheatDetected),
        ),
        (
            GameStatus::NoStart,
            None,
            finished(GameOutcome::Aborted, Ending::NeverStarted),
        ),
        (
            GameStatus::UnknownFinish,
            None,
            finished(GameOutcome::Undecided, Ending::Unknown),
        ),
        (
            GameStatus::InsufficientMaterialClaim,
            None,
            finished(GameOutcome::Draw, Ending::InsufficientMaterialClaimed),
        ),
        (
            GameStatus::VariantEnd,
            white,
            finished(GameOutcome::WhiteWon, Ending::VariantEnd),
        ),
    ];
    for (status, winner, phase) in cases {
        assert_eq!(
            GamePhase::of(status, winner),
            phase,
            "{status:?} {winner:?}"
        );
    }
}

#[test]
fn a_created_game_is_not_an_event() {
    let created = GameState {
        game: alice_vs_bob("g1"),
        status: GameStatus::Created,
        winner: None,
    };
    assert_eq!(GameEvent::of(created), None);
}
