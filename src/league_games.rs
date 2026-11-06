mod config;

use std::collections::BTreeSet;
use std::convert::Infallible;
use std::num::NonZeroUsize;
use std::sync::Arc;

use kameo::Actor;
use kameo::actor::ActorRef;
use kameo::message::{Context, Message, StreamMessage};
use tokio::sync::{Semaphore, broadcast};
use url::Url;

use crate::game_monitor::{
    self, Decision, Difference, Draw, Fixture, GameChange, GameEvent, VoidReason, Win,
};
use crate::heltour::{Heltour, PairingResult, PairingUpdate, UpdatePairingError, league};

pub use config::Config;

#[derive(Clone, Debug)]
pub struct Reporter {
    heltour: Heltour,
    lichess: Url,
    parallel_reports: Arc<Semaphore>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Breach {
    Terms(BTreeSet<Difference>),
    Decision(Decision),
    Void(VoidReason),
}

impl Reporter {
    #[must_use]
    pub fn new(heltour: Heltour, lichess: Url, parallel_reports: NonZeroUsize) -> Self {
        let permits = parallel_reports.get().min(Semaphore::MAX_PERMITS);
        Self {
            heltour,
            lichess,
            parallel_reports: Arc::new(Semaphore::new(permits)),
        }
    }

    async fn report_in_parallel(&self, event: GameEvent) {
        let permit = Arc::clone(&self.parallel_reports).acquire_owned().await;
        let reporter = self.clone();
        tokio::spawn(async move {
            reporter.report(&event).await;
            drop(permit);
        });
    }

    async fn report(&self, event: &GameEvent) {
        let game = event.game();
        for matched in &event.matched {
            let Some(league) = league::Tag::of_monitored(&matched.league) else {
                continue;
            };
            let result = match judge(&event.change, &matched.fixture) {
                Ok(result) => result,
                Err(breach) => {
                    tracing::info!(game = %game.id, league = %league, ?breach, "game not reported to Heltour");
                    continue;
                }
            };
            let update = PairingUpdate {
                league: league.clone(),
                white: game.white.user_id.clone(),
                black: game.black.user_id.clone(),
                game_link: game.id.link(&self.lichess),
                result,
            };
            match self.heltour.update_pairing(&update).await {
                Ok(()) => {
                    tracing::info!(game = %game.id, league = %league, ?result, "game reported to Heltour");
                }
                Err(UpdatePairingError::Refused(refusal)) => {
                    tracing::info!(game = %game.id, league = %league, ?refusal, "Heltour declined the game");
                }
                Err(error) => {
                    tracing::warn!(error = %error, cause = ?std::error::Error::source(&error), game = %game.id, "could not report the game to Heltour");
                }
            }
        }
    }
}

fn judge(change: &GameChange, fixture: &Fixture) -> Result<Option<PairingResult>, Breach> {
    let broken = broken_terms(fixture, change);
    if !broken.is_empty() {
        return Err(Breach::Terms(broken));
    }
    match change {
        GameChange::Started(_) => Ok(None),
        GameChange::Decided { decision, .. } => result_of(*decision)
            .map(Some)
            .ok_or(Breach::Decision(*decision)),
        GameChange::Void { reason, .. } => Err(Breach::Void(*reason)),
    }
}

fn broken_terms(fixture: &Fixture, change: &GameChange) -> BTreeSet<Difference> {
    let mut differences = fixture.differences_from(change.game());
    differences.remove(&Difference::ColorsFlipped);
    differences
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
            self.report_in_parallel(event).await;
        }
    }
}

#[cfg(test)]
mod tests;
