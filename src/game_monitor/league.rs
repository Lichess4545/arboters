use std::fmt;

use nutype::nutype;

#[nutype(derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deref, Display))]
pub struct Source(String);

#[nutype(derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deref, Display))]
pub struct Key(String);

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct League {
    pub source: Source,
    pub key: Key,
}

impl fmt::Display for League {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}/{}", self.source, self.key)
    }
}
