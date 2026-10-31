use std::collections::BTreeMap;

use kameo::Actor;
use kameo::actor::{ActorRef, Recipient};
use kameo::message::{Context, Message};
use rand::rngs::StdRng;
use rand::seq::IndexedRandom;
use tokio::time::Instant;

use crate::chat::{Conversation, Messenger, OutgoingMessage, Overhear, OverheardMessage};

use super::cooldown::Cooldown;
use super::model::Response;
use super::settings::ResponsesSettings;
use super::{InboxUnavailable, Responses};

#[derive(Debug)]
pub struct Responder<R, M> {
    responses: R,
    messenger: M,
    cooldown: Cooldown,
    choices: StdRng,
}

impl<R: Responses, M: Messenger> Responder<R, M> {
    #[must_use]
    pub fn new(responses: R, messenger: M, settings: &ResponsesSettings, choices: StdRng) -> Self {
        Self {
            responses,
            messenger,
            cooldown: Cooldown::new(settings.cooldown),
            choices,
        }
    }

    async fn answer(&mut self, message: OverheardMessage) {
        let responses = match self.responses.list(None).await {
            Ok(responses) => responses,
            Err(error) => {
                tracing::error!(error = %error, cause = ?std::error::Error::source(&error), "could not look up responses");
                return;
            }
        };
        let mut by_phrase: BTreeMap<String, Vec<Response>> = BTreeMap::new();
        for response in responses {
            if response.phrase.appears_in(&message.text, response.matching) {
                by_phrase
                    .entry(response.phrase.ignoring_case())
                    .or_default()
                    .push(response);
            }
        }
        let now = Instant::now();
        let topic = Conversation::Topic {
            channel: message.channel.clone(),
            topic: message.topic.clone(),
        };
        for choices in by_phrase.values() {
            let Some(chosen) = choices.choose(&mut self.choices) else {
                continue;
            };
            if !self
                .cooldown
                .fires(&message.channel, &message.topic, &chosen.phrase, now)
            {
                continue;
            }
            let reply = OutgoingMessage::new(chosen.markdown.as_str().to_owned());
            if let Err(error) = self.messenger.send(&topic, &reply).await {
                tracing::warn!(
                    error = %error,
                    cause = %error.cause,
                    response_id = chosen.id.0,
                    "could not post a response"
                );
            }
        }
    }
}

impl<R: Responses, M: Messenger> Actor for Responder<R, M> {
    type Args = (Self, Recipient<Overhear>);
    type Error = InboxUnavailable;

    async fn on_start(
        (responder, inbox): Self::Args,
        actor_ref: ActorRef<Self>,
    ) -> Result<Self, Self::Error> {
        inbox
            .tell(Overhear(actor_ref.recipient::<OverheardMessage>()))
            .await?;
        Ok(responder)
    }
}

impl<R: Responses, M: Messenger> Message<OverheardMessage> for Responder<R, M> {
    type Reply = ();

    async fn handle(&mut self, message: OverheardMessage, _: &mut Context<Self, Self::Reply>) {
        self.answer(message).await;
    }
}

#[cfg(test)]
mod tests;
