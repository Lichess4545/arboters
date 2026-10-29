use crate::chat::Notification;
use crate::game_monitor::{GameEvent, GameOutcome};
use crate::heltour::team_pairing::{BoardNumber, HalfPoints};
use crate::lichess::Game;

use super::team_match::{MatchResult, Side, TeamMatch};

#[must_use]
pub fn render(event: &GameEvent, team_match: Option<&TeamMatch<'_>>) -> Notification {
    let announcement = match event {
        GameEvent::Started(game) => format!("{} has started: {}", pairing(game), game.id.link()),
        GameEvent::Finished { game, outcome, .. } => match outcome.score() {
            Some(score) => format!(
                "{} is over. The result is {score}: {}",
                pairing(game),
                game.id.link()
            ),
            None if *outcome == GameOutcome::Aborted => {
                format!("{} was aborted: {}", pairing(game), game.id.link())
            }
            None => format!("{} is over: {}", pairing(game), game.id.link()),
        },
    };
    let markdown = match team_match {
        Some(team_match) => format!("{announcement}\n{}", match_context(event, team_match)),
        None => announcement,
    };
    Notification::new(markdown)
}

fn pairing(game: &Game) -> String {
    format!(
        "@_**{}** vs @_**{}**",
        game.white.user_id, game.black.user_id
    )
}

fn match_context(event: &GameEvent, team_match: &TeamMatch<'_>) -> String {
    let mut sentences = vec![standing(event, team_match)];
    if let Some(boards) = boards_in_play(&team_match.boards_in_play) {
        sentences.push(boards);
    }
    if team_match.this_game_can_decide {
        sentences.push("This game can decide the match.".to_owned());
    }
    sentences.join(" ")
}

fn standing(event: &GameEvent, team_match: &TeamMatch<'_>) -> String {
    let now = match event {
        GameEvent::Started(_) => "",
        GameEvent::Finished { .. } => "now ",
    };
    let score = team_match.standing.score;
    let (white, black) = (team_match.white_team, team_match.black_team);
    let (white_points, black_points) = (points(score.white), points(score.black));
    let (leader, trailer, leading, trailing) = match score.leader() {
        Some(Side::White) | None => (white, black, &white_points, &black_points),
        Some(Side::Black) => (black, white, &black_points, &white_points),
    };
    let games_left = team_match.standing.unfinished_boards > 0;
    match (team_match.standing.settled(), games_left) {
        (Some(MatchResult::Won(_)), false) => {
            format!("{leader} win the match against {trailer} {leading}–{trailing}.")
        }
        (Some(MatchResult::Won(_)), true) => {
            format!("{leader} lead {trailer} {leading}–{trailing} and have won the match.")
        }
        (Some(MatchResult::Drawn), _) => {
            format!("{white} and {black} draw the match {white_points}–{black_points}.")
        }
        (None, _) if score.leader().is_none() => {
            format!("{white} and {black} are {now}tied {white_points}–{black_points}.")
        }
        (None, _) => format!("{leader} {now}lead {trailer} {leading}–{trailing}."),
    }
}

fn boards_in_play(boards: &[BoardNumber]) -> Option<String> {
    match boards {
        [] => None,
        [board] => Some(format!("Board {board} is also being played.")),
        [others @ .., last] => {
            let others: Vec<_> = others.iter().map(ToString::to_string).collect();
            Some(format!(
                "Boards {} and {last} are also being played.",
                others.join(", ")
            ))
        }
    }
}

fn points(half_points: HalfPoints) -> String {
    let halves = half_points.into_inner();
    match (halves / 2, halves % 2) {
        (0, 1) => "½".to_owned(),
        (whole, 0) => whole.to_string(),
        (whole, _) => format!("{whole}½"),
    }
}

impl GameOutcome {
    const fn score(self) -> Option<&'static str> {
        match self {
            Self::WhiteWon => Some("1-0"),
            Self::BlackWon => Some("0-1"),
            Self::Draw => Some("½-½"),
            Self::Aborted | Self::Undecided => None,
        }
    }
}

#[cfg(test)]
mod tests;
