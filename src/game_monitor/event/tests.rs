use super::*;
use crate::testing::alice_vs_bob;

fn game() -> Game {
    alice_vs_bob("g1")
}

fn change_of(status: GameStatus) -> Option<GameChange> {
    GameChange::of(GameState {
        game: game(),
        status,
    })
}

fn decided(decision: Decision) -> GameChange {
    GameChange::Decided {
        game: game(),
        decision,
    }
}

fn white_won(win: Win) -> GameChange {
    decided(Decision::WhiteWon(win))
}

fn black_won(win: Win) -> GameChange {
    decided(Decision::BlackWon(win))
}

fn drawn(draw: Draw) -> GameChange {
    decided(Decision::Drawn(draw))
}

fn void(reason: VoidReason) -> GameChange {
    GameChange::Void {
        game: game(),
        reason,
    }
}

#[test]
fn statuses_map_to_game_changes() {
    let white = Color::White;
    let black = Color::Black;
    let cases = [
        (GameStatus::Started, GameChange::Started(game())),
        (GameStatus::Aborted, void(VoidReason::Aborted)),
        (
            GameStatus::Mate { winner: white },
            white_won(Win::Checkmate),
        ),
        (
            GameStatus::Resign { winner: black },
            black_won(Win::Resignation),
        ),
        (GameStatus::Stalemate, drawn(Draw::Stalemate)),
        (
            GameStatus::Timeout {
                winner: Some(white),
            },
            white_won(Win::VictoryClaimed),
        ),
        (
            GameStatus::Timeout { winner: None },
            drawn(Draw::PlayerLeft),
        ),
        (GameStatus::Draw, drawn(Draw::AgreementOrRule)),
        (
            GameStatus::Outoftime { winner: None },
            drawn(Draw::OutOfTime),
        ),
        (
            GameStatus::Outoftime {
                winner: Some(black),
            },
            black_won(Win::OutOfTime),
        ),
        (
            GameStatus::Cheat { winner: black },
            black_won(Win::CheatDetected),
        ),
        (
            GameStatus::NoStart { winner: white },
            void(VoidReason::NeverStarted),
        ),
        (
            GameStatus::UnknownFinish { winner: None },
            drawn(Draw::Unknown),
        ),
        (
            GameStatus::UnknownFinish {
                winner: Some(white),
            },
            white_won(Win::Unknown),
        ),
        (
            GameStatus::InsufficientMaterialClaim,
            drawn(Draw::InsufficientMaterialClaimed),
        ),
        (
            GameStatus::VariantEnd { winner: white },
            white_won(Win::VariantEnd),
        ),
    ];
    for (status, change) in cases {
        assert_eq!(change_of(status), Some(change), "{status:?}");
    }
}

#[test]
fn a_created_game_is_not_a_change() {
    assert_eq!(change_of(GameStatus::Created), None);
}
