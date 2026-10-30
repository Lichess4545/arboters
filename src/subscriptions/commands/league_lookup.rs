use crate::heltour::{ActiveLeagues, League};
use crate::lichess::Clock;

use super::parsing::WrittenLeague;
use super::spelling::{Closest, closest};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Found<'a> {
    One(&'a League),
    Several(Vec<&'a League>),
    Nothing,
}

pub fn find<'a>(active: &'a ActiveLeagues, written: &WrittenLeague) -> Found<'a> {
    let leagues = &active.leagues;
    let written = written.as_str();
    if let Some(league) = leagues.iter().find(|league| league.tag.as_str() == written) {
        return Found::One(league);
    }
    if let Some(league) = leagues
        .iter()
        .find(|league| same_words(league.name.as_str(), written))
    {
        return Found::One(league);
    }
    if let Some(clock) = time_control(written) {
        let mut timed: Vec<&League> = leagues
            .iter()
            .filter(|league| league.time_control == Some(clock))
            .collect();
        return match (timed.pop(), timed.is_empty()) {
            (None, _) => Found::Nothing,
            (Some(league), true) => Found::One(league),
            (Some(league), false) => {
                timed.push(league);
                Found::Several(timed)
            }
        };
    }
    match closest(
        written,
        leagues.iter().map(|league| (league.tag.as_str(), league)),
    ) {
        Closest::One(league) => Found::One(league),
        Closest::Tie(leagues) => Found::Several(leagues),
        Closest::Nothing => Found::Nothing,
    }
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
