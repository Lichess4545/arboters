use super::*;
use crate::game_monitor::GameEvent;
use crate::lichess::GameState;
use crate::testing::alice_vs_bob;

const fn finished(outcome: GameOutcome, ending: Ending) -> GamePhase {
    GamePhase::Finished { outcome, ending }
}

#[test]
fn statuses_map_to_game_phases() {
    let white = Color::White;
    let black = Color::Black;
    let cases = [
        (GameStatus::Created, GamePhase::Created),
        (GameStatus::Started, GamePhase::Started),
        (
            GameStatus::Aborted,
            finished(GameOutcome::Aborted, Ending::Aborted),
        ),
        (
            GameStatus::Mate { winner: white },
            finished(GameOutcome::WhiteWon, Ending::Checkmate),
        ),
        (
            GameStatus::Resign { winner: black },
            finished(GameOutcome::BlackWon, Ending::Resignation),
        ),
        (
            GameStatus::Stalemate,
            finished(GameOutcome::Draw, Ending::Stalemate),
        ),
        (
            GameStatus::Timeout {
                winner: Some(white),
            },
            finished(GameOutcome::WhiteWon, Ending::VictoryClaimed),
        ),
        (GameStatus::Draw, finished(GameOutcome::Draw, Ending::Draw)),
        (
            GameStatus::Outoftime { winner: None },
            finished(GameOutcome::Draw, Ending::OutOfTime),
        ),
        (
            GameStatus::Outoftime {
                winner: Some(black),
            },
            finished(GameOutcome::BlackWon, Ending::OutOfTime),
        ),
        (
            GameStatus::Cheat { winner: black },
            finished(GameOutcome::BlackWon, Ending::CheatDetected),
        ),
        (
            GameStatus::NoStart { winner: white },
            finished(GameOutcome::Aborted, Ending::NeverStarted),
        ),
        (
            GameStatus::UnknownFinish { winner: None },
            finished(GameOutcome::Undecided, Ending::Unknown),
        ),
        (
            GameStatus::InsufficientMaterialClaim,
            finished(GameOutcome::Draw, Ending::InsufficientMaterialClaimed),
        ),
        (
            GameStatus::VariantEnd { winner: white },
            finished(GameOutcome::WhiteWon, Ending::VariantEnd),
        ),
    ];
    for (status, phase) in cases {
        assert_eq!(GamePhase::of(status), phase, "{status:?}");
    }
}

#[test]
fn a_created_game_is_not_an_event() {
    let created = GameState {
        game: alice_vs_bob("g1"),
        status: GameStatus::Created,
    };
    assert_eq!(GameEvent::of(created), None);
}
