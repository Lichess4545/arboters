use super::*;
use crate::game_monitor::Ending;
use crate::game_monitor::LichessSettings;
use crate::lichess::{self, GameId, Player, Speed, Variant};

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
        variant: Variant::Standard,
        clock: None,
    }
}

fn lichess() -> Url {
    LichessSettings::default().base_url
}

fn finished(outcome: GameOutcome, ending: Ending) -> String {
    render(
        &GameEvent::Finished {
            game: game(),
            outcome,
            ending,
        },
        &lichess(),
    )
    .markdown()
    .to_owned()
}

#[test]
fn a_started_game_announces_white_vs_black_and_the_game_link() {
    assert_eq!(
        render(&GameEvent::Started(game()), &lichess()).markdown(),
        "@_**alice** vs @_**bob** has started: https://lichess.org/game/abcd1234"
    );
}

#[test]
fn a_finished_game_with_a_result_announces_its_chess_score() {
    assert_eq!(
        finished(GameOutcome::WhiteWon, Ending::Checkmate),
        "@_**alice** vs @_**bob** is over. The result is 1-0: https://lichess.org/game/abcd1234"
    );
    assert_eq!(
        finished(GameOutcome::BlackWon, Ending::Resignation),
        "@_**alice** vs @_**bob** is over. The result is 0-1: https://lichess.org/game/abcd1234"
    );
    assert_eq!(
        finished(GameOutcome::Draw, Ending::Draw),
        "@_**alice** vs @_**bob** is over. The result is ½-½: https://lichess.org/game/abcd1234"
    );
}

#[test]
fn an_aborted_game_is_announced_as_aborted() {
    assert_eq!(
        finished(GameOutcome::Aborted, Ending::Aborted),
        "@_**alice** vs @_**bob** was aborted: https://lichess.org/game/abcd1234"
    );
}

#[test]
fn an_undecided_game_is_announced_as_over_without_a_result() {
    assert_eq!(
        finished(GameOutcome::Undecided, Ending::Unknown),
        "@_**alice** vs @_**bob** is over: https://lichess.org/game/abcd1234"
    );
}

#[test]
fn the_game_link_points_at_the_configured_lichess_site() {
    let local = Url::parse("http://localhost:9663").unwrap();
    assert_eq!(
        render(&GameEvent::Started(game()), &local).markdown(),
        "@_**alice** vs @_**bob** has started: http://localhost:9663/game/abcd1234"
    );
}
