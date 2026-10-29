//! League concepts as heltour defines them in `heltour/tournament/models.py`.

mod active_leagues;
mod api;
pub mod league;
mod pairing;
pub mod team;
pub mod team_pairing;

pub use active_leagues::ActiveLeagues;
pub use api::{
    ActiveLeaguesError, Heltour, HeltourSettings, PairingResult, PairingUpdate, Refusal,
    TeamPairingsError, UpdatePairingError,
};
pub use league::League;
pub use pairing::Pairing;
pub use team_pairing::TeamPairing;
