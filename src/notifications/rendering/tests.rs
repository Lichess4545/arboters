use super::*;
use crate::lichess::{self, GameId, Player, Speed};

fn game() -> Game {
    let player = |name| Player {
        user_id: lichess::UserId::try_new(name).unwrap(),
        rating: None,
    };
    Game {
        id: GameId::new("abcd1234"),
        white: player("alice"),
        black: player("bob"),
        speed: Speed::Classical,
        rated: true,
    }
}

fn finished(outcome: GameOutcome) -> String {
    render(&GameEvent::Finished {
        game: game(),
        outcome,
    })
    .markdown()
    .to_owned()
}

#[test]
fn a_started_game_announces_white_vs_black_and_the_game_link() {
    assert_eq!(
        render(&GameEvent::Started(game())).markdown(),
        "**alice** vs **bob** has started: https://lichess.org/abcd1234"
    );
}

#[test]
fn a_finished_game_with_a_result_announces_its_chess_score() {
    assert_eq!(
        finished(GameOutcome::WhiteWon),
        "**alice** vs **bob** is over. The result is 1-0: https://lichess.org/abcd1234"
    );
    assert_eq!(
        finished(GameOutcome::BlackWon),
        "**alice** vs **bob** is over. The result is 0-1: https://lichess.org/abcd1234"
    );
    assert_eq!(
        finished(GameOutcome::Draw),
        "**alice** vs **bob** is over. The result is ½-½: https://lichess.org/abcd1234"
    );
}

#[test]
fn an_aborted_game_is_announced_as_aborted() {
    assert_eq!(
        finished(GameOutcome::Aborted),
        "**alice** vs **bob** was aborted: https://lichess.org/abcd1234"
    );
}

#[test]
fn an_undecided_game_is_announced_as_over_without_a_result() {
    assert_eq!(
        finished(GameOutcome::Undecided),
        "**alice** vs **bob** is over: https://lichess.org/abcd1234"
    );
}
