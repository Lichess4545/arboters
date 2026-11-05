use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub host: IpAddr,
    pub port: u16,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: 8080,
        }
    }
}

impl Config {
    #[must_use]
    pub const fn address(&self) -> SocketAddr {
        SocketAddr::new(self.host, self.port)
    }
}
