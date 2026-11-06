use url::Url;

use crate::chat::OutgoingMessage;
use crate::game_monitor::{Decision, GameEvent};

pub fn render(event: &GameEvent, lichess: &Url) -> OutgoingMessage {
    let game = event.game();
    let link = game.id.link(lichess);
    let players = format!(
        "@_**{}** vs @_**{}**",
        game.white.user_id, game.black.user_id
    );
    OutgoingMessage::new(match event {
        GameEvent::Started(_) => format!("{players} has started: {link}"),
        GameEvent::Decided { decision, .. } => match decision {
            Decision::WhiteWon(_) => format!("{players} is over. The result is 1-0: {link}"),
            Decision::BlackWon(_) => format!("{players} is over. The result is 0-1: {link}"),
            Decision::Drawn(_) => format!("{players} is over. The result is ½-½: {link}"),
        },
        GameEvent::Void { .. } => format!("{players} was aborted: {link}"),
    })
}

#[cfg(test)]
mod tests;
