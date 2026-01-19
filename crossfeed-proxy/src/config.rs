use crossfeed_storage::BodyLimits;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProxyConfig {
    pub listen: ListenConfig,
    pub tls: TlsMitmConfig,
    pub upstream: UpstreamConfig,
    pub scope: ScopeConfig,
    pub body_limits: BodyLimits,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ListenConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TlsMitmConfig {
    pub enabled: bool,
    pub allow_legacy: bool,
    pub ca_common_name: String,
    pub ca_cert_dir: String,
    pub leaf_cert_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpstreamConfig {
    pub mode: UpstreamMode,
    pub socks: Option<SocksConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum UpstreamMode {
    Direct,
    Socks,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SocksConfig {
    pub host: String,
    pub port: u16,
    pub version: SocksVersion,
    pub auth: SocksAuthConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SocksVersion {
    V4,
    V4a,
    V5,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SocksAuthConfig {
    None,
    UserPass { username: String, password: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScopeConfig {
    pub rules: Vec<ScopeRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScopeRule {
    pub rule_type: ScopeRuleType,
    pub pattern_type: ScopePatternType,
    pub target: ScopeTarget,
    pub pattern: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ScopeRuleType {
    Include,
    Exclude,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ScopePatternType {
    Wildcard,
    Regex,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ScopeTarget {
    Host,
    Path,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            listen: ListenConfig {
                host: "127.0.0.1".to_string(),
                port: 8080,
            },
            tls: TlsMitmConfig {
                enabled: true,
                allow_legacy: false,
                ca_common_name: "Crossfeed Proxy CA".to_string(),
                ca_cert_dir: "certs".to_string(),
                leaf_cert_dir: "certs/leaf".to_string(),
            },
            upstream: UpstreamConfig {
                mode: UpstreamMode::Direct,
                socks: None,
            },
            scope: ScopeConfig { rules: Vec::new() },
            body_limits: BodyLimits::default(),
        }
    }
}
