use super::league::League;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ActiveLeagues {
    pub leagues: Vec<League>,
}
