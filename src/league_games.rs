use kameo::Actor;
use kameo::actor::{ActorRef, Recipient};
use kameo::message::{Context, Message};
use url::Url;

use crate::game_monitor::{Ending, Follow, GameEvent, GameOutcome, MonitorUnavailable};
use crate::heltour::{
    ActiveLeagues, Heltour, League, PairingResult, PairingUpdate, UpdatePairingError,
};

#[derive(Debug)]
pub struct Reporter {
    heltour: Heltour,
    lichess: Url,
    active: ActiveLeagues,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Breach {
    TimeControl,
    Rated,
    Variant,
    Ending(Ending),
}

impl Reporter {
    #[must_use]
    pub fn new(heltour: Heltour, lichess: Url) -> Self {
        Self {
            heltour,
            lichess,
            active: ActiveLeagues::default(),
        }
    }

    async fn report(&self, event: &GameEvent) {
        let game = event.game();
        let paired_in = self.active.leagues_of(game);
        let leagues = self
            .active
            .leagues
            .iter()
            .filter(|league| paired_in.contains(&league.tag));
        for league in leagues {
            let result = match judge(event, league) {
                Ok(result) => result,
                Err(breach) => {
                    tracing::info!(game = %game.id, league = %league.tag, ?breach, "game not reported to heltour");
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
                    tracing::info!(game = %game.id, league = %league.tag, ?result, "game reported to heltour");
                }
                Err(UpdatePairingError::Refused(refusal)) => {
                    tracing::info!(game = %game.id, league = %league.tag, ?refusal, "heltour declined the game");
                }
                Err(error) => {
                    tracing::warn!(error = %error, cause = ?std::error::Error::source(&error), game = %game.id, "could not report the game to heltour");
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
        GameEvent::Finished {
            outcome, ending, ..
        } => result_of(*outcome, *ending).map(Some),
    }
}

const fn result_of(outcome: GameOutcome, ending: Ending) -> Result<PairingResult, Breach> {
    match ending {
        Ending::VictoryClaimed
        | Ending::CheatDetected
        | Ending::Aborted
        | Ending::NeverStarted
        | Ending::Unknown => Err(Breach::Ending(ending)),
        Ending::Checkmate
        | Ending::Resignation
        | Ending::Stalemate
        | Ending::Draw
        | Ending::OutOfTime
        | Ending::InsufficientMaterialClaimed
        | Ending::VariantEnd => match outcome {
            GameOutcome::WhiteWon => Ok(PairingResult::WhiteWon),
            GameOutcome::BlackWon => Ok(PairingResult::BlackWon),
            GameOutcome::Draw => Ok(PairingResult::Draw),
            GameOutcome::Aborted | GameOutcome::Undecided => Err(Breach::Ending(ending)),
        },
    }
}

impl Actor for Reporter {
    type Args = (Self, Recipient<Follow<GameEvent>>);
    type Error = MonitorUnavailable;

    async fn on_start(
        (reporter, monitor): Self::Args,
        actor_ref: ActorRef<Self>,
    ) -> Result<Self, Self::Error> {
        monitor
            .tell(Follow(actor_ref.recipient::<GameEvent>()))
            .await?;
        Ok(reporter)
    }
}

impl Message<ActiveLeagues> for Reporter {
    type Reply = ();

    async fn handle(&mut self, active: ActiveLeagues, _: &mut Context<Self, Self::Reply>) {
        self.active = active;
    }
}

impl Message<GameEvent> for Reporter {
    type Reply = ();

    async fn handle(&mut self, event: GameEvent, _: &mut Context<Self, Self::Reply>) {
        self.report(&event).await;
    }
}

#[cfg(test)]
mod tests;
