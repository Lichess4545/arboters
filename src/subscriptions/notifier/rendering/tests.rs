use super::*;
use crate::game_monitor::Ending;
use crate::heltour::team;
use crate::lichess::{self, GameId, Player, Speed, Variant};
use crate::subscriptions::notifier::team_match::{Score, Standing};

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

fn finished(outcome: GameOutcome, ending: Ending) -> String {
    render(
        &GameEvent::Finished {
            game: game(),
            outcome,
            ending,
        },
        None,
    )
    .markdown()
    .to_owned()
}

#[test]
fn a_started_game_announces_white_vs_black_and_the_game_link() {
    assert_eq!(
        render(&GameEvent::Started(game()), None).markdown(),
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
fn points_are_written_with_a_half_sign() {
    let written: Vec<_> = [0, 1, 2, 5, 12]
        .into_iter()
        .map(|halves| points(HalfPoints::new(halves)))
        .collect();
    assert_eq!(written, ["0", "½", "1", "2½", "6"]);
}

struct Teams {
    knights: team::DisplayName,
    rooks: team::DisplayName,
}

impl Teams {
    fn new() -> Self {
        Self {
            knights: team::DisplayName::try_new("Knights").unwrap(),
            rooks: team::DisplayName::try_new("Rooks").unwrap(),
        }
    }

    fn at(
        &self,
        (white_halves, black_halves): (u16, u16),
        unfinished_boards: u16,
        boards_in_play: &[u16],
        this_game_can_decide: bool,
    ) -> TeamMatch<'_> {
        TeamMatch {
            white_team: &self.knights,
            black_team: &self.rooks,
            standing: Standing {
                score: Score {
                    white: HalfPoints::new(white_halves),
                    black: HalfPoints::new(black_halves),
                },
                unfinished_boards,
            },
            boards_in_play: boards_in_play
                .iter()
                .map(|&number| BoardNumber::new(number))
                .collect(),
            this_game_can_decide,
        }
    }
}

fn started_in(team_match: &TeamMatch<'_>) -> String {
    render(&GameEvent::Started(game()), Some(team_match))
        .markdown()
        .to_owned()
}

fn finished_in(team_match: &TeamMatch<'_>) -> String {
    render(
        &GameEvent::Finished {
            game: game(),
            outcome: GameOutcome::WhiteWon,
            ending: Ending::Resignation,
        },
        Some(team_match),
    )
    .markdown()
    .to_owned()
}

const STARTED: &str = "@_**alice** vs @_**bob** has started: https://lichess.org/game/abcd1234";
const FINISHED: &str =
    "@_**alice** vs @_**bob** is over. The result is 1-0: https://lichess.org/game/abcd1234";

#[test]
fn a_started_team_game_adds_the_score_the_boards_in_play_and_whether_it_can_decide() {
    let teams = Teams::new();
    assert_eq!(
        started_in(&teams.at((5, 3), 3, &[3, 5], true)),
        format!(
            "{STARTED}\nKnights lead Rooks 2½–1½. Boards 3 and 5 are also being played. This game can decide the match."
        )
    );
    assert_eq!(
        started_in(&teams.at((1, 3), 6, &[2], false)),
        format!("{STARTED}\nRooks lead Knights 1½–½. Board 2 is also being played.")
    );
    assert_eq!(
        started_in(&teams.at((0, 0), 8, &[], false)),
        format!("{STARTED}\nKnights and Rooks are tied 0–0.")
    );
    assert_eq!(
        started_in(&teams.at((6, 0), 2, &[2, 3, 5], false)),
        format!(
            "{STARTED}\nKnights lead Rooks 3–0 and have won the match. Boards 2, 3 and 5 are also being played."
        )
    );
}

#[test]
fn a_finished_team_game_adds_the_score_now_or_the_match_result() {
    let teams = Teams::new();
    assert_eq!(
        finished_in(&teams.at((7, 3), 3, &[6], false)),
        format!("{FINISHED}\nKnights now lead Rooks 3½–1½. Board 6 is also being played.")
    );
    assert_eq!(
        finished_in(&teams.at((4, 4), 2, &[], false)),
        format!("{FINISHED}\nKnights and Rooks are now tied 2–2.")
    );
    assert_eq!(
        finished_in(&teams.at((4, 8), 0, &[], false)),
        format!("{FINISHED}\nRooks win the match against Knights 4–2.")
    );
    assert_eq!(
        finished_in(&teams.at((6, 6), 0, &[], false)),
        format!("{FINISHED}\nKnights and Rooks draw the match 3–3.")
    );
}
