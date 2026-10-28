use serde::Deserialize;

use crate::lichess;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Pairing {
    pub white: lichess::UserId,
    pub black: lichess::UserId,
}
