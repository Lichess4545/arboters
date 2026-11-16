mod parsing;

use std::collections::HashMap;
use std::convert::Infallible;
use std::time::Duration;

use kameo::Actor;
use kameo::actor::{ActorRef, Spawn};
use kameo::message::{Context, Message};
use url::Url;

use crate::chat::{ChatUserId, IncomingMessage, MessageId, OutgoingMessage};
use crate::commands::{Handling, Listener};

use super::Config;
use super::link::LinkMistake;
use super::model::{NewResponse, Response, ResponseId};
use super::phrase::{Matching, Phrase};
use super::{MessageReader, ReadError, Responses, ResponsesError};
use parsing::{Mistake, parse};

const PREVIEW_LENGTH: usize = 60;
const REFUSAL: &str = "Only moderators can manage saved responses.";
const FAILURE: &str = "Something went wrong on my side. Please try again later.";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Help,
    ResponseHelp,
    Add {
        phrase: Phrase,
        matching: Matching,
        source: MessageId,
    },
    List(Option<Phrase>),
    Remove(ResponseId),
    Mistake(Mistake),
    Refused,
}

#[derive(Debug)]
pub struct ResponseCommand {
    sender: ChatUserId,
    message: MessageId,
    action: Action,
}

impl ResponseCommand {
    fn new(message: &IncomingMessage, action: Option<Action>) -> Option<Self> {
        let action = match action? {
            Action::Help if !message.role.moderates() => return None,
            _ if !message.role.moderates() => Action::Refused,
            action => action,
        };
        Some(Self {
            sender: message.sender,
            message: message.id,
            action,
        })
    }
}

#[derive(Debug)]
pub struct ResponseCommands<R, L> {
    responses: R,
    reader: L,
    site: Url,
    cooldown: Duration,
    added_by: HashMap<MessageId, ResponseId>,
}

impl<R: Responses, L: MessageReader> ResponseCommands<R, L> {
    #[must_use]
    pub fn new(responses: R, reader: L, site: Url, config: &Config) -> Self {
        Self {
            responses,
            reader,
            site,
            cooldown: config.cooldown,
            added_by: HashMap::new(),
        }
    }

    async fn answer(&mut self, command: ResponseCommand) -> String {
        let outcome = match command.action {
            Action::Help | Action::ResponseHelp => Ok(self.help()),
            Action::Add {
                phrase,
                matching,
                source,
            } => {
                self.add(phrase, matching, source, command.sender, command.message)
                    .await
            }
            Action::List(phrase) => self.list(phrase.as_ref()).await,
            Action::Remove(id) => self.remove(id).await,
            Action::Mistake(mistake) => Ok(self.explain(mistake)),
            Action::Refused => Ok(REFUSAL.to_owned()),
        };
        outcome.unwrap_or_else(|error| {
            tracing::error!(error = %error, cause = ?std::error::Error::source(&error), "response command failed");
            FAILURE.to_owned()
        })
    }

    async fn add(
        &mut self,
        phrase: Phrase,
        matching: Matching,
        source: MessageId,
        sender: ChatUserId,
        message: MessageId,
    ) -> Result<String, ResponsesError> {
        let markdown = match self.reader.read(source).await {
            Ok(markdown) => markdown,
            Err(ReadError::Unreadable) => {
                return Ok("I can't read that message. It may have been deleted, or it may be in a private channel that I am not subscribed to.".to_owned());
            }
            Err(ReadError::Failed(cause)) => {
                tracing::warn!(cause = %cause, message_id = source.0, "could not read a linked message");
                return Ok(FAILURE.to_owned());
            }
        };
        let preview = preview(&markdown);
        let id = self
            .responses
            .add(NewResponse {
                phrase: phrase.clone(),
                matching,
                markdown,
                source,
                added_by: sender,
            })
            .await?;
        let replaced = match self.added_by.insert(message, id) {
            Some(earlier) if earlier != id => {
                self.responses.remove(earlier).await?.then_some(earlier)
            }
            _ => None,
        };
        let replacement = replaced
            .map(|earlier| format!(" It replaces response `{earlier}`."))
            .unwrap_or_default();
        Ok(format!(
            "Response `{id}` answers {} with {preview}.{replacement}",
            phrase_shown(&phrase, matching)
        ))
    }

    async fn list(&self, phrase: Option<&Phrase>) -> Result<String, ResponsesError> {
        let responses = self.responses.list(phrase).await?;
        if responses.is_empty() {
            return Ok(phrase.map_or_else(
                || "There are no saved responses.".to_owned(),
                |phrase| format!("There are no saved responses to {}.", code(phrase)),
            ));
        }
        let lines: Vec<String> = responses.iter().map(line).collect();
        Ok(format!("Saved responses:\n{}", lines.join("\n")))
    }

    async fn remove(&self, id: ResponseId) -> Result<String, ResponsesError> {
        Ok(if self.responses.remove(id).await? {
            format!("Removed response `{id}`.")
        } else {
            format!("There is no response `{id}`. Use `response list` to see the ids.")
        })
    }

    fn help(&self) -> String {
        format!(
            "As a moderator, you can also save responses to phrases. When a saved phrase appears in a channel message, I answer in the same topic with one of the phrase's responses, chosen at random.\n\
             - `response add \"<phrase>\" <message link>` saves a copy of the linked message as a response to the phrase. Later edits to that message do not change the copy.\n\
             - `response add exact \"<phrase>\" <message link>` does the same, but the phrase then matches only in exactly the case you type it.\n\
             - `response list` lists the saved responses and their ids. Add `\"<phrase>\"` to list the responses to one phrase.\n\
             - `response remove <id>` removes a response.\n\
             \n\
             A phrase matches whole words, in any case unless it was added with `exact`. After I answer a phrase in a topic, I wait {} before I answer it in that topic again.",
            humantime::format_duration(self.cooldown)
        )
    }

    fn explain(&self, mistake: Mistake) -> String {
        match mistake {
            Mistake::Unknown => self.help(),
            Mistake::Add => "Use `response add \"<phrase>\" <message link>`, with the phrase in double quotes, or `response add exact \"<phrase>\" <message link>` to match only that case.".to_owned(),
            Mistake::List => "Use `response list`, or `response list \"<phrase>\"` for the responses to one phrase.".to_owned(),
            Mistake::Remove => "Use `response remove <id>`. `response list` shows the ids.".to_owned(),
            Mistake::EmptyPhrase => "The phrase between the quotes is empty.".to_owned(),
            Mistake::Link(LinkMistake::NotAMessageLink) => "That is not a link to a message. To copy one, open the message's menu and choose **Copy link to message**.".to_owned(),
            Mistake::Link(LinkMistake::OtherSite) => format!("That link is to another site. Link to a message on {}.", self.site),
        }
    }
}

fn line(response: &Response) -> String {
    format!(
        "- `{}` {}: {}",
        response.id,
        phrase_shown(&response.phrase, response.matching),
        preview(&response.markdown)
    )
}

fn phrase_shown(phrase: &Phrase, matching: Matching) -> String {
    match matching {
        Matching::AnyCase => code(phrase),
        Matching::ExactCase => format!("{} (exact case)", code(phrase)),
    }
}

fn code(text: &str) -> String {
    format!("`{}`", text.replace('`', "'"))
}

fn preview(markdown: &str) -> String {
    let first_line = markdown
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default();
    let mut shown: String = first_line.chars().take(PREVIEW_LENGTH).collect();
    if shown.len() < first_line.len() || markdown.trim() != first_line {
        shown.push('\u{2026}');
    }
    code(&shown)
}

impl<R: Responses, L: MessageReader> ResponseCommands<R, L> {
    pub fn listen(self) -> (ActorRef<Self>, Listener) {
        let site = self.site.clone();
        let response_commands = Self::spawn(self);
        let listener = Listener::new(
            move |message: &IncomingMessage| {
                ResponseCommand::new(message, parse(&message.text, &site))
            },
            response_commands
                .clone()
                .reply_recipient::<ResponseCommand>(),
        );
        (response_commands, listener)
    }
}

impl<R: Responses, L: MessageReader> Actor for ResponseCommands<R, L> {
    type Args = Self;
    type Error = Infallible;

    fn on_start(
        response_commands: Self,
        _: ActorRef<Self>,
    ) -> impl Future<Output = Result<Self, Self::Error>> + Send {
        std::future::ready(Ok(response_commands))
    }
}

impl<R: Responses, L: MessageReader> Message<ResponseCommand> for ResponseCommands<R, L> {
    type Reply = Handling;

    async fn handle(
        &mut self,
        command: ResponseCommand,
        _: &mut Context<Self, Self::Reply>,
    ) -> Handling {
        Handling::Answered(OutgoingMessage::new(self.answer(command).await))
    }
}

#[cfg(test)]
mod tests;
