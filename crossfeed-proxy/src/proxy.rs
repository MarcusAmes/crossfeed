use crate::config::ProxyConfig;

#[derive(Debug)]
pub struct Proxy {
    pub config: ProxyConfig,
}

impl Proxy {
    pub fn new(config: ProxyConfig) -> Self {
        Self { config }
    }
}
