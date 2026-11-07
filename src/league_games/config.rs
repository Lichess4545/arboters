use std::num::NonZeroUsize;

use serde::Deserialize;

const PARALLEL_REPORTS: NonZeroUsize = NonZeroUsize::new(8).unwrap();

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub report: bool,
    pub parallel_reports: NonZeroUsize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            report: false,
            parallel_reports: PARALLEL_REPORTS,
        }
    }
}
