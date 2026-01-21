#![recursion_limit = "512"]

mod config;
mod error;
mod events;
mod intercept;
mod proxy;
mod scope;
mod timeline_event;

pub use config::{
    ListenConfig, ProxyConfig, ProxyProtocolMode, ScopeConfig, ScopePatternType, ScopeRule,
    ScopeRuleType, ScopeTarget, SocksAuthConfig, SocksConfig, SocksVersion, TlsMitmConfig,
    UpstreamConfig, UpstreamMode,
};
pub use error::ProxyError;
pub use events::{ProxyCommand, ProxyControl, ProxyEvents, control_channel, event_channel};
pub use intercept::{InterceptDecision, InterceptManager, InterceptResult};
pub use proxy::Proxy;
pub use scope::is_in_scope;
pub use timeline_event::{ProxyEvent, ProxyEventKind};

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
