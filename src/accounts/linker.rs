use kameo::Actor;
use kameo::actor::{ActorRef, Recipient};
use kameo::message::{Context, Message};

use crate::chat::{ChatUserId, Joined, WatchJoins};
use crate::heltour::{FindPlayerError, Heltour};
use crate::lichess;

use super::{Accounts, InboxUnavailable};

#[derive(Debug)]
pub struct Linker<A> {
    accounts: A,
    heltour: Heltour,
}

enum Identity {
    Player(lichess::UserId),
    Unknown,
}

impl<A: Accounts> Linker<A> {
    #[must_use]
    pub const fn new(accounts: A, heltour: Heltour) -> Self {
        Self { accounts, heltour }
    }

    async fn welcome(&self, joined: Joined) {
        match self.identity_of(&joined).await {
            Ok(Identity::Player(player)) => self.link(joined.user, &player).await,
            Ok(Identity::Unknown) => {}
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

impl<A: Accounts> Actor for Linker<A> {
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

impl<A: Accounts> Message<Joined> for Linker<A> {
    type Reply = ();

    async fn handle(&mut self, joined: Joined, _: &mut Context<Self, Self::Reply>) {
        self.welcome(joined).await;
    }
}

#[cfg(test)]
mod tests;
