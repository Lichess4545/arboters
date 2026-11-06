mod config;

use std::convert::Infallible;

use kameo::Actor;
use kameo::actor::ActorRef;
use kameo::message::{Context, Message, StreamMessage};
use tokio::sync::{broadcast, watch};
use url::Url;

use crate::game_monitor::{self, Decision, Draw, GameEvent, VoidReason, Win};
use crate::heltour::{
    ActiveLeagues, Heltour, League, PairingResult, PairingUpdate, UpdatePairingError,
};

pub use config::Config;

#[derive(Debug)]
pub struct Reporter {
    heltour: Heltour,
    lichess: Url,
    active_leagues: watch::Receiver<ActiveLeagues>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Breach {
    TimeControl,
    Rated,
    Variant,
    Decision(Decision),
    Void(VoidReason),
}

impl Reporter {
    #[must_use]
    pub const fn new(
        heltour: Heltour,
        lichess: Url,
        active_leagues: watch::Receiver<ActiveLeagues>,
    ) -> Self {
        Self {
            heltour,
            lichess,
            active_leagues,
        }
    }

    fn leagues_pairing_the_players_of(&self, event: &GameEvent) -> Vec<League> {
        let active = self.active_leagues.borrow();
        let paired_in = active.leagues_of(event.game());
        active
            .leagues
            .iter()
            .filter(|league| paired_in.contains(&league.tag))
            .cloned()
            .collect()
    }

    async fn report(&self, event: &GameEvent) {
        let game = event.game();
        for league in self.leagues_pairing_the_players_of(event) {
            let result = match judge(event, &league) {
                Ok(result) => result,
                Err(breach) => {
                    tracing::info!(game = %game.id, league = %league.tag, ?breach, "game not reported to Heltour");
                    continue;
                }
            };
            let update = PairingUpdate {
                league: league.tag.clone(),
                white: game.white.user_id.clone(),
                black: game.black.user_id.clone(),
                game_link: game.id.link(&self.lichess),
                result,
            };
            match self.heltour.update_pairing(&update).await {
                Ok(()) => {
                    tracing::info!(game = %game.id, league = %league.tag, ?result, "game reported to Heltour");
                }
                Err(UpdatePairingError::Refused(refusal)) => {
                    tracing::info!(game = %game.id, league = %league.tag, ?refusal, "Heltour declined the game");
                }
                Err(error) => {
                    tracing::warn!(error = %error, cause = ?std::error::Error::source(&error), game = %game.id, "could not report the game to Heltour");
                }
            }
        }
    }
}

fn judge(event: &GameEvent, league: &League) -> Result<Option<PairingResult>, Breach> {
    let game = event.game();
    if league.time_control.is_none() || game.clock != league.time_control {
        return Err(Breach::TimeControl);
    }
    if game.rated != league.rated {
        return Err(Breach::Rated);
    }
    if game.variant != league.variant {
        return Err(Breach::Variant);
    }
    match event {
        GameEvent::Started(_) => Ok(None),
        GameEvent::Decided { decision, .. } => result_of(*decision)
            .map(Some)
            .ok_or(Breach::Decision(*decision)),
        GameEvent::Void { reason, .. } => Err(Breach::Void(*reason)),
    }
}

const fn result_of(decision: Decision) -> Option<PairingResult> {
    match decision {
        Decision::WhiteWon(win) if counts(win) => Some(PairingResult::WhiteWon),
        Decision::BlackWon(win) if counts(win) => Some(PairingResult::BlackWon),
        Decision::Drawn(draw) if counts_as_draw(draw) => Some(PairingResult::Draw),
        Decision::WhiteWon(_) | Decision::BlackWon(_) | Decision::Drawn(_) => None,
    }
}

const fn counts(win: Win) -> bool {
    match win {
        Win::Checkmate | Win::Resignation | Win::OutOfTime | Win::VariantEnd => true,
        Win::VictoryClaimed | Win::CheatDetected | Win::Unknown => false,
    }
}

const fn counts_as_draw(draw: Draw) -> bool {
    match draw {
        Draw::AgreementOrRule
        | Draw::Stalemate
        | Draw::InsufficientMaterialClaimed
        | Draw::OutOfTime => true,
        Draw::PlayerLeft | Draw::Unknown => false,
    }
}

impl Actor for Reporter {
    type Args = (Self, broadcast::Receiver<GameEvent>);
    type Error = Infallible;

    fn on_start(
        (reporter, events): Self::Args,
        actor_ref: ActorRef<Self>,
    ) -> impl Future<Output = Result<Self, Self::Error>> + Send {
        actor_ref.attach_stream(game_monitor::event_stream(events), (), ());
        std::future::ready(Ok(reporter))
    }
}

impl Message<StreamMessage<GameEvent, (), ()>> for Reporter {
    type Reply = ();

    async fn handle(
        &mut self,
        message: StreamMessage<GameEvent, (), ()>,
        _: &mut Context<Self, Self::Reply>,
    ) {
        if let StreamMessage::Next(event) = message {
            self.report(&event).await;
        }
    }
}

#[cfg(test)]
mod tests;
