use crate::heltour::{ActiveLeagues, League};
use crate::lichess::Clock;

use super::parsing::WrittenLeague;
use super::spelling::{Found, closest};

pub fn find<'a>(active: &'a ActiveLeagues, written: &WrittenLeague) -> Found<&'a League> {
    let leagues = &active.leagues;
    let written = written.as_str();
    let named = leagues
        .iter()
        .find(|league| league.tag.as_str() == written)
        .or_else(|| {
            leagues
                .iter()
                .find(|league| same_words(league.name.as_str(), written))
        });
    if let Some(league) = named {
        return Found::One(league);
    }
    if let Some(clock) = time_control(written) {
        return leagues
            .iter()
            .filter(|league| league.time_control == Some(clock))
            .collect::<Vec<_>>()
            .into();
    }
    closest(
        written,
        leagues.iter().map(|league| (league.tag.as_str(), league)),
    )
}

fn same_words(name: &str, written: &str) -> bool {
    name.split_whitespace()
        .map(str::to_lowercase)
        .eq(written.split_whitespace().map(str::to_lowercase))
}

fn time_control(written: &str) -> Option<Clock> {
    let (minutes, increment) = written.split_once('+')?;
    Some(Clock {
        initial_seconds: minutes.trim().parse::<u32>().ok()?.checked_mul(60)?,
        increment_seconds: increment.trim().parse().ok()?,
    })
}

#[cfg(test)]
mod tests;
