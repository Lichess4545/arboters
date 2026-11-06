mod config;

use std::collections::BTreeSet;
use std::time::Duration;

use kameo::actor::Recipient;
use time::OffsetDateTime;
use tokio::sync::{oneshot, watch};
use tokio::time::MissedTickBehavior;

use crate::game_monitor::{Fixture, NewFixtures, Period, SubmitFixtures};

use super::league;
use super::pairing::{Pairing, Round};
use super::{ActiveLeagues, Heltour, League, ReadError};

pub use config::Config;

#[derive(Debug)]
struct MonitorStopped;

pub async fn keep_in_sync(
    heltour: Heltour,
    monitor: Recipient<SubmitFixtures>,
    active_leagues: watch::Sender<ActiveLeagues>,
    refresh_interval: Duration,
) {
    let mut refreshes = tokio::time::interval(refresh_interval);
    refreshes.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        refreshes.tick().await;
        if refresh(&heltour, &monitor, &active_leagues, refresh_interval)
            .await
            .is_err()
        {
            tracing::info!("game monitor has stopped; ending heltour sync");
            return;
        }
    }
}

async fn refresh(
    heltour: &Heltour,
    monitor: &Recipient<SubmitFixtures>,
    active_leagues: &watch::Sender<ActiveLeagues>,
    timeout: Duration,
) -> Result<(), MonitorStopped> {
    let leagues = match heltour.active_leagues(timeout).await {
        Ok(leagues) => leagues,
        Err(error) => {
            tracing::warn!(error = ?error, "keeping the previous leagues and fixtures");
            return Ok(());
        }
    };
    tracing::debug!(leagues = leagues.leagues.len(), "read the active leagues");
    let current = leagues.leagues.clone();
    active_leagues.send_replace(leagues);
    for league in &current {
        refresh_fixtures_of(heltour, monitor, league, timeout).await?;
    }
    Ok(())
}

async fn refresh_fixtures_of(
    heltour: &Heltour,
    monitor: &Recipient<SubmitFixtures>,
    league: &League,
    timeout: Duration,
) -> Result<(), MonitorStopped> {
    match current_fixtures(heltour, league, timeout).await {
        Ok(Some(fixtures)) => submit(monitor, fixtures).await,
        Ok(None) => {
            tracing::debug!(league = %league.tag, "no current round");
            Ok(())
        }
        Err(error) => {
            tracing::warn!(error = ?error, league = %league.tag, "keeping the previous fixtures");
            Ok(())
        }
    }
}

async fn current_fixtures(
    heltour: &Heltour,
    league: &League,
    timeout: Duration,
) -> Result<Option<NewFixtures>, ReadError> {
    let rounds = current_rounds(heltour, &league.tag, timeout).await?;
    if rounds.is_empty() {
        return Ok(None);
    }
    let mut games = BTreeSet::new();
    for round in &rounds {
        let pairings = heltour.pairings(&league.tag, round.id, timeout).await?;
        games.extend(
            pairings
                .into_iter()
                .map(|pairing| fixture_of(pairing, league)),
        );
    }
    Ok(Some(NewFixtures {
        league: league.tag.monitored(),
        period: period_of(rounds.iter().copied()),
        games,
    }))
}

async fn current_rounds(
    heltour: &Heltour,
    league: &league::Tag,
    timeout: Duration,
) -> Result<Vec<Round>, ReadError> {
    let now = OffsetDateTime::now_utc();
    let rounds = heltour.active_rounds(league, timeout).await?;
    Ok(rounds
        .into_iter()
        .filter(|round| !round.has_ended_by(now))
        .collect())
}

fn fixture_of(pairing: Pairing, league: &League) -> Fixture {
    Fixture {
        white: pairing.white,
        black: pairing.black,
        time_control: league.time_control,
        variant: league.variant,
        rated: league.rated,
    }
}

fn period_of(rounds: impl Iterator<Item = Round> + Clone) -> Period {
    Period {
        starts: earliest(rounds.clone().map(|round| round.starts)),
        ends: latest(rounds.map(|round| round.ends)),
    }
}

fn earliest(times: impl Iterator<Item = Option<OffsetDateTime>>) -> Option<OffsetDateTime> {
    times.collect::<Option<Vec<_>>>()?.into_iter().min()
}

fn latest(times: impl Iterator<Item = Option<OffsetDateTime>>) -> Option<OffsetDateTime> {
    times.collect::<Option<Vec<_>>>()?.into_iter().max()
}

async fn submit(
    monitor: &Recipient<SubmitFixtures>,
    fixtures: NewFixtures,
) -> Result<(), MonitorStopped> {
    let league = fixtures.league.clone();
    let (stored, id) = oneshot::channel();
    monitor
        .tell(SubmitFixtures { fixtures, stored })
        .await
        .map_err(|_| MonitorStopped)?;
    if let Ok(id) = id.await {
        tracing::debug!(league = %league, fixtures = %id, "fixtures submitted");
    } else {
        tracing::warn!(league = %league, "the game monitor did not store the fixtures");
    }
    Ok(())
}

#[cfg(test)]
mod tests;
