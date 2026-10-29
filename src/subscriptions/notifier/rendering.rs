use crate::chat::Notification;
use crate::game_monitor::{GameEvent, GameOutcome};
use crate::lichess::Game;

#[must_use]
pub fn render(event: &GameEvent) -> Notification {
    let markdown = match event {
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
    Notification::new(markdown)
}

fn pairing(game: &Game) -> String {
    format!(
        "@_**{}** vs @_**{}**",
        game.white.user_id, game.black.user_id
    )
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
