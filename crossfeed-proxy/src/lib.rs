mod config;
mod error;
mod events;
mod proxy;
mod scope;
mod timeline_event;

pub use config::{
    ListenConfig, ProxyConfig, ScopeConfig, ScopePatternType, ScopeRule, ScopeRuleType, ScopeTarget,
    SocksAuthConfig, SocksConfig, SocksVersion, TlsMitmConfig, UpstreamConfig, UpstreamMode,
};
pub use error::ProxyError;
pub use events::{event_channel, ProxyEvents};
pub use proxy::Proxy;
pub use scope::is_in_scope;
pub use timeline_event::TimelineEvent;

#[cfg(test)]
mod tests {
    use super::ProxyConfig;

    #[test]
    fn default_config_is_local() {
        let config = ProxyConfig::default();
        assert_eq!(config.listen.host, "127.0.0.1");
        assert_eq!(config.listen.port, 8080);
        assert!(config.tls.enabled);
    }
}
