use super::*;
use crate::heltour::team;
use crate::heltour::team_pairing::{BoardNumber, HalfPoints};
use crate::lichess::{GameId, Player, Speed, UserId, Variant};

fn user_id(name: &str) -> UserId {
    UserId::try_new(name).unwrap()
}

fn tag(tag: &str) -> Tag {
    Tag::try_new(tag).unwrap()
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

fn pairing(league: &str, white: &str, black: &str) -> Pairing {
    Pairing {
        league: tag(league),
        white: user_id(white),
        black: user_id(black),
    }
}

fn team_pairing(league: &str, boards: &[(u16, &str, &str)]) -> TeamPairing {
    let team = |name: &str| team::DisplayName::try_new(name).unwrap();
    TeamPairing {
        league: tag(league),
        white_team: team("Knights"),
        white_points: HalfPoints::ZERO,
        black_team: team("Rooks"),
        black_points: HalfPoints::ZERO,
        boards: boards
            .iter()
            .map(|&(number, white, black)| Board {
                number: BoardNumber::new(number),
                white: Some(user_id(white)),
                black: Some(user_id(black)),
                white_team: team("Knights"),
                black_team: team("Rooks"),
                game_link: None,
                result: None,
            })
            .collect(),
    }
}

fn board_number_of(active: &ActiveLeagues, game: &lichess::Game) -> Option<BoardNumber> {
    active.team_board_of(game).map(|(_, board)| board.number)
}

#[test]
fn a_game_is_on_the_one_board_that_seats_its_players_in_either_color() {
    let active = ActiveLeagues {
        pairings: vec![pairing("team4545", "alice", "bob")],
        team_pairings: vec![team_pairing(
            "team4545",
            &[(1, "carol", "dave"), (2, "alice", "bob")],
        )],
        ..ActiveLeagues::default()
    };

    assert_eq!(
        board_number_of(&active, &game("bob", "alice")),
        Some(BoardNumber::new(2))
    );
}

#[test]
fn a_game_is_on_no_board_of_a_team_pairing_in_another_league() {
    let active = ActiveLeagues {
        pairings: vec![pairing("lonewolf", "alice", "bob")],
        team_pairings: vec![team_pairing("team4545", &[(1, "alice", "bob")])],
        ..ActiveLeagues::default()
    };

    assert_eq!(board_number_of(&active, &game("alice", "bob")), None);
}

#[test]
fn a_game_whose_players_share_more_than_one_board_is_on_none() {
    let active = ActiveLeagues {
        pairings: vec![
            pairing("team4545", "alice", "bob"),
            pairing("team4546", "alice", "bob"),
        ],
        team_pairings: vec![
            team_pairing("team4545", &[(1, "alice", "bob")]),
            team_pairing("team4546", &[(3, "alice", "bob")]),
        ],
        ..ActiveLeagues::default()
    };

    assert_eq!(board_number_of(&active, &game("alice", "bob")), None);
}
