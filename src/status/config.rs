use serde::Deserialize;

use crate::chat::ChannelName;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub channel: ChannelName,
}
