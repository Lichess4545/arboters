use kameo::actor::Recipient;

use super::conversation::ChatUserId;
use super::email::Email;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Joined {
    pub user: ChatUserId,
    pub email: Option<Email>,
}

#[derive(Debug)]
pub struct WatchJoins(pub Recipient<Joined>);
