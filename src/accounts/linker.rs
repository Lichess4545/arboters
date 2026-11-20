use kameo::Actor;
use kameo::actor::{ActorRef, Recipient};
use kameo::message::{Context, Message};

use crate::chat::{ChatUserId, Conversation, Joined, Messenger, OutgoingMessage, WatchJoins};
use crate::heltour::{FindPlayerError, Heltour};
use crate::lichess;

use super::{Accounts, InboxUnavailable};

const UNIDENTIFIED_WELCOME: &str = "Welcome! To take part in the leagues, your Zulip account needs to be linked to your Lichess account, and we couldn't work out which Lichess account is yours. Please reach out to the moderators.";

#[derive(Debug)]
pub struct Linker<A, M> {
    accounts: A,
    heltour: Heltour,
    messenger: M,
}

enum Identity {
    Player(lichess::UserId),
    Unknown,
}

impl<A: Accounts, M: Messenger> Linker<A, M> {
    #[must_use]
    pub const fn new(accounts: A, heltour: Heltour, messenger: M) -> Self {
        Self {
            accounts,
            heltour,
            messenger,
        }
    }

    async fn welcome(&self, joined: Joined) {
        match self.identity_of(&joined).await {
            Ok(Identity::Player(player)) => self.link(joined.user, &player).await,
            Ok(Identity::Unknown) => self.ask_to_reach_the_moderators(joined.user).await,
            Err(error) => {
                tracing::warn!(error = %error, cause = ?std::error::Error::source(&error), user = joined.user.0, "could not ask heltour for the player of a user who joined");
            }
        }
    }

    async fn identity_of(&self, joined: &Joined) -> Result<Identity, FindPlayerError> {
        let user = joined.user.0;
        let Some(email) = &joined.email else {
            tracing::info!(user, "a user joined whose real email the bot cannot see");
            return Ok(Identity::Unknown);
        };
        match self.heltour.player_with_email(email).await {
            Ok(Some(player)) => Ok(Identity::Player(player)),
            Ok(None) => {
                tracing::info!(user, "no heltour player has the email of a user who joined");
                Ok(Identity::Unknown)
            }
            Err(FindPlayerError::Ambiguous) => {
                tracing::info!(
                    user,
                    "more than one heltour player has the email of a user who joined"
                );
                Ok(Identity::Unknown)
            }
            Err(error) => Err(error),
        }
    }

    async fn ask_to_reach_the_moderators(&self, user: ChatUserId) {
        let conversation = Conversation::Direct { user_id: user };
        let welcome = OutgoingMessage::new(UNIDENTIFIED_WELCOME.to_owned());
        if let Err(error) = self.messenger.send(&conversation, &welcome).await {
            tracing::warn!(error = %error, cause = %error.cause, user = user.0, "could not welcome a user who joined unidentified");
        }
    }

    async fn link(&self, user: ChatUserId, player: &lichess::UserId) {
        match self.accounts.link(user, player).await {
            Ok(()) => {
                tracing::info!(user = user.0, lichess = %player, "linked a user who joined to their Lichess account");
            }
            Err(error) => {
                tracing::error!(error = %error, cause = ?std::error::Error::source(&error), user = user.0, lichess = %player, "could not link a user who joined to their Lichess account");
            }
        }
    }
}

impl<A: Accounts, M: Messenger> Actor for Linker<A, M> {
    type Args = (Self, Recipient<WatchJoins>);
    type Error = InboxUnavailable;

    async fn on_start(
        (linker, inbox): Self::Args,
        actor_ref: ActorRef<Self>,
    ) -> Result<Self, Self::Error> {
        inbox
            .tell(WatchJoins(actor_ref.recipient::<Joined>()))
            .await?;
        Ok(linker)
    }
}

impl<A: Accounts, M: Messenger> Message<Joined> for Linker<A, M> {
    type Reply = ();

    async fn handle(&mut self, joined: Joined, _: &mut Context<Self, Self::Reply>) {
        self.welcome(joined).await;
    }
}

#[cfg(test)]
mod tests;
