use crate::game_monitor::{GameEvent, GameOutcome};
use crate::lichess::Game;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notification {
    markdown: String,
}

impl Notification {
    /// ```
    /// use arboters::notifications::Notification;
    ///
    /// assert_eq!(Notification::new("**hello**".to_owned()).markdown(), "**hello**");
    /// ```
    #[must_use]
    pub const fn new(markdown: String) -> Self {
        Self { markdown }
    }

    #[must_use]
    pub fn markdown(&self) -> &str {
        &self.markdown
    }
}

#[must_use]
pub fn render(event: &GameEvent) -> Notification {
    let markdown = match event {
        GameEvent::Started(game) => format!("{} has started: {}", pairing(game), game.id.link()),
        GameEvent::Finished { game, outcome } => match outcome.score() {
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
    Notification { markdown }
}

fn pairing(game: &Game) -> String {
    format!("@_**{}** vs @_**{}**", game.white.user_id, game.black.user_id)
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
