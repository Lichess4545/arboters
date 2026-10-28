use std::collections::BTreeSet;

use crate::games::LichessUsername;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Watch(pub BTreeSet<LichessUsername>);
