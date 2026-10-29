use url::Url;

use super::*;
use crate::game_monitor::Ending;
use crate::heltour::league;
use crate::heltour::team_pairing::BoardResult;
use crate::lichess::{GameId, Player, Speed, Variant};

fn standing(white_halves: u16, black_halves: u16, unfinished_boards: u16) -> Standing {
    Standing {
        score: Score {
            white: HalfPoints::new(white_halves),
            black: HalfPoints::new(black_halves),
        },
        unfinished_boards,
    }
}

#[test]
fn a_match_is_settled_once_the_unfinished_boards_cannot_change_its_result() {
    let cases = [
        ("4 boards, none played", standing(0, 0, 4), None),
        ("1-2, one left", standing(2, 4, 1), None),
        ("2-1, one left", standing(4, 2, 1), None),
        ("3-0, three left", standing(6, 0, 3), None),
        (
            "3-0, one left",
            standing(6, 0, 1),
            Some(MatchResult::Won(Side::White)),
        ),
        (
            "½-2, one left",
            standing(1, 4, 1),
            Some(MatchResult::Won(Side::Black)),
        ),
        (
            "4-2, all played",
            standing(8, 4, 0),
            Some(MatchResult::Won(Side::White)),
        ),
        (
            "3-3, all played",
            standing(6, 6, 0),
            Some(MatchResult::Drawn),
        ),
    ];
    for (case, standing, settled) in cases {
        assert_eq!(standing.settled(), settled, "{case}");
    }
}

#[test]
fn a_game_can_decide_the_match_when_one_of_its_results_would_settle_it() {
    let cases = [
        ("6 boards at 2½-2½, one left", standing(5, 5, 1), true),
        ("4 boards at 2-0, two left", standing(4, 0, 2), true),
        ("6 boards at 2½-1½, two left", standing(5, 3, 2), true),
        (
            "4 boards at 2-1, one left, a loss draws",
            standing(4, 2, 1),
            true,
        ),
        ("4 boards at 1½-1½, one left", standing(3, 3, 1), true),
        ("4 boards at 1-1, two left", standing(2, 2, 2), false),
        ("6 boards at 3-0, three left", standing(6, 0, 3), true),
        ("8 boards at 2-2, four left", standing(4, 4, 4), false),
        ("4 boards, none played", standing(0, 0, 4), false),
        ("already won at 3-0, one left", standing(6, 0, 1), false),
        ("already won at 2½-½, one left", standing(5, 1, 1), false),
        ("all played", standing(4, 4, 0), false),
    ];
    for (case, standing, deciding) in cases {
        assert_eq!(standing.one_game_can_settle(), deciding, "{case}");
    }
}

fn user_id(name: &str) -> lichess::UserId {
    lichess::UserId::try_new(name).unwrap()
}

fn team(name: &str) -> team::DisplayName {
    team::DisplayName::try_new(name).unwrap()
}

fn board(number: u16, white: &str, black: &str, result: Option<BoardResult>) -> Board {
    let (white_team, black_team) = if number % 2 == 1 {
        ("Knights", "Rooks")
    } else {
        ("Rooks", "Knights")
    };
    Board {
        number: BoardNumber::new(number),
        white: Some(user_id(white)),
        black: Some(user_id(black)),
        white_team: team(white_team),
        black_team: team(black_team),
        game_link: None,
        result,
    }
}

fn in_play(board: Board) -> Board {
    Board {
        game_link: Some(Url::parse("https://lichess.org/game/abcd1234").unwrap()),
        ..board
    }
}

fn knights_vs_rooks() -> TeamPairing {
    TeamPairing {
        league: league::Tag::try_new("team4545").unwrap(),
        white_team: team("Knights"),
        white_points: HalfPoints::new(3),
        black_team: team("Rooks"),
        black_points: HalfPoints::new(1),
        boards: vec![
            in_play(board(1, "knight1", "rook1", Some(BoardResult::WhiteWon))),
            board(2, "rook2", "knight2", Some(BoardResult::Draw)),
            in_play(board(3, "knight3", "rook3", None)),
            board(4, "rook4", "knight4", None),
            in_play(board(5, "knight5", "rook5", None)),
            board(6, "rook6", "knight6", None),
        ],
    }
}

fn game(white: &str, black: &str) -> lichess::Game {
    let player = |name| Player {
        user_id: user_id(name),
        rating: None,
    };
    lichess::Game {
        id: GameId::new("abcd1234"),
        white: player(white),
        black: player(black),
        speed: Speed::Classical,
        rated: true,
        variant: Variant::Standard,
        clock: None,
    }
}

fn finished(white: &str, black: &str, outcome: GameOutcome) -> GameEvent {
    GameEvent::Finished {
        game: game(white, black),
        outcome,
        ending: Ending::Resignation,
    }
}

fn board_number(pairing: &TeamPairing, number: u16) -> &Board {
    pairing
        .boards
        .iter()
        .find(|board| board.number == BoardNumber::new(number))
        .unwrap()
}

#[test]
fn a_started_game_shows_heltours_score_and_the_other_boards_in_play() {
    let pairing = knights_vs_rooks();
    let team_match = TeamMatch::of(
        &GameEvent::Started(game("rook4", "knight4")),
        &pairing,
        board_number(&pairing, 4),
    )
    .unwrap();

    assert_eq!(team_match.white_team, &team("Knights"));
    assert_eq!(team_match.black_team, &team("Rooks"));
    assert_eq!(team_match.standing, standing(3, 1, 4));
    assert_eq!(
        team_match.boards_in_play,
        [BoardNumber::new(3), BoardNumber::new(5)]
    );
    assert!(!team_match.this_game_can_decide);
}

#[test]
fn a_started_game_that_heltour_already_has_a_result_for_cannot_decide_the_match() {
    let mut pairing = knights_vs_rooks();
    pairing.boards.truncate(3);
    pairing
        .boards
        .push(board(4, "rook4", "knight4", Some(BoardResult::Draw)));
    let team_match = TeamMatch::of(
        &GameEvent::Started(game("rook4", "knight4")),
        &pairing,
        board_number(&pairing, 4),
    )
    .unwrap();

    assert_eq!(team_match.standing, standing(3, 1, 1));
    assert!(!team_match.this_game_can_decide);
}

#[test]
fn a_finished_game_heltour_has_no_result_for_yet_is_added_to_its_teams_score() {
    let pairing = knights_vs_rooks();
    let cases = [
        ("rook4", "knight4", GameOutcome::WhiteWon, standing(3, 3, 3)),
        ("rook4", "knight4", GameOutcome::BlackWon, standing(5, 1, 3)),
        ("knight4", "rook4", GameOutcome::WhiteWon, standing(5, 1, 3)),
        ("knight4", "rook4", GameOutcome::Draw, standing(4, 2, 3)),
    ];
    for (white, black, outcome, expected) in cases {
        let team_match = TeamMatch::of(
            &finished(white, black, outcome),
            &pairing,
            board_number(&pairing, 4),
        )
        .unwrap();
        assert_eq!(
            team_match.standing, expected,
            "{white} vs {black}, {outcome:?}"
        );
        assert!(!team_match.this_game_can_decide);
    }
}

#[test]
fn a_finished_game_heltour_has_a_result_for_keeps_heltours_score() {
    let pairing = knights_vs_rooks();
    let team_match = TeamMatch::of(
        &finished("rook2", "knight2", GameOutcome::Draw),
        &pairing,
        board_number(&pairing, 2),
    )
    .unwrap();

    assert_eq!(team_match.standing, standing(3, 1, 4));
}

#[test]
fn a_finished_game_without_a_result_has_no_team_match() {
    let pairing = knights_vs_rooks();
    for outcome in [GameOutcome::Aborted, GameOutcome::Undecided] {
        assert_eq!(
            TeamMatch::of(
                &finished("rook4", "knight4", outcome),
                &pairing,
                board_number(&pairing, 4),
            ),
            None
        );
    }
}

#[test]
fn a_board_in_play_has_a_game_link_and_no_result_and_is_not_this_game() {
    let pairing = knights_vs_rooks();
    let team_match = TeamMatch::of(
        &GameEvent::Started(game("knight3", "rook3")),
        &pairing,
        board_number(&pairing, 3),
    )
    .unwrap();

    assert_eq!(team_match.boards_in_play, [BoardNumber::new(5)]);
}
