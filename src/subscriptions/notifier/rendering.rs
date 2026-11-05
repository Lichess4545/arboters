use url::Url;

use crate::chat::OutgoingMessage;
use crate::game_monitor::{GameEvent, GameOutcome};

pub fn render(event: &GameEvent, lichess: &Url) -> OutgoingMessage {
    let game = event.game();
    let link = game.id.link(lichess);
    let players = format!(
        "@_**{}** vs @_**{}**",
        game.white.user_id, game.black.user_id
    );
    OutgoingMessage::new(match event {
        GameEvent::Started(_) => format!("{players} has started: {link}"),
        GameEvent::Finished { outcome, .. } => match outcome {
            GameOutcome::WhiteWon => format!("{players} is over. The result is 1-0: {link}"),
            GameOutcome::BlackWon => format!("{players} is over. The result is 0-1: {link}"),
            GameOutcome::Draw => format!("{players} is over. The result is ½-½: {link}"),
            GameOutcome::Aborted => format!("{players} was aborted: {link}"),
            GameOutcome::Undecided => format!("{players} is over: {link}"),
        },
    })
}

#[cfg(test)]
mod tests;
