use std::collections::HashMap;
use std::time::Duration;

use tokio::time::Instant;

use crate::chat::{ChannelName, TopicName};

use super::phrase::Phrase;

#[derive(Debug)]
pub struct Cooldown {
    wait: Duration,
    fired: HashMap<Spot, Instant>,
}

#[derive(Debug, PartialEq, Eq, Hash)]
struct Spot {
    channel: ChannelName,
    topic: TopicName,
    phrase: String,
}

impl Cooldown {
    pub fn new(wait: Duration) -> Self {
        Self {
            wait,
            fired: HashMap::new(),
        }
    }

    pub fn fires(
        &mut self,
        channel: &ChannelName,
        topic: &TopicName,
        phrase: &Phrase,
        now: Instant,
    ) -> bool {
        let wait = self.wait;
        self.fired
            .retain(|_, fired| now.saturating_duration_since(*fired) < wait);
        let spot = Spot {
            channel: channel.clone(),
            topic: topic.clone(),
            phrase: phrase.ignoring_case(),
        };
        if self.fired.contains_key(&spot) {
            return false;
        }
        self.fired.insert(spot, now);
        true
    }
}

#[cfg(test)]
mod tests;
