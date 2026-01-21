use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ProjectConfig {
    pub timeline: TimelineConfig,
    pub proxy: ProxyProjectConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct TimelineConfig {
    pub body_limits_mb: BodyLimitsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ProxyProjectConfig {
    pub listen_host: String,
    pub listen_port: u16,
    pub protocol_mode: ProxyProtocolMode,
    pub http1_max_header_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ProxyProtocolMode {
    Auto,
    Http1,
    Http2,
}

impl Default for ProxyProtocolMode {
    fn default() -> Self {
        Self::Auto
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct BodyLimitsConfig {
    pub request_max_mb: u64,
    pub response_max_mb: u64,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            timeline: TimelineConfig::default(),
            proxy: ProxyProjectConfig::default(),
        }
    }
}

impl Default for TimelineConfig {
    fn default() -> Self {
        Self {
            body_limits_mb: BodyLimitsConfig::default(),
        }
    }
}

impl Default for BodyLimitsConfig {
    fn default() -> Self {
        Self {
            request_max_mb: 40,
            response_max_mb: 40,
        }
    }
}

impl Default for ProxyProjectConfig {
    fn default() -> Self {
        Self {
            listen_host: "127.0.0.1".to_string(),
            listen_port: 8888,
            protocol_mode: ProxyProtocolMode::Auto,
            http1_max_header_bytes: 256 * 1024,
        }
    }
}

impl ProjectConfig {
    pub fn load_or_create(path: &Path) -> Result<Self, String> {
        if path.exists() {
            let raw = std::fs::read_to_string(path).map_err(|err| err.to_string())?;
            toml::from_str(&raw).map_err(|err| err.to_string())
        } else {
            let config = Self::default();
            config.save(path)?;
            Ok(config)
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let contents = toml::to_string_pretty(self).map_err(|err| err.to_string())?;
        std::fs::write(path, contents).map_err(|err| err.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectLayout {
    pub config_filename: String,
    pub database_filename: String,
    pub exports_dirname: String,
    pub logs_dirname: String,
}

impl Default for ProjectLayout {
    fn default() -> Self {
        Self {
            config_filename: "project.toml".to_string(),
            database_filename: "crossfeed.db".to_string(),
            exports_dirname: "exports".to_string(),
            logs_dirname: "logs".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectPaths {
    pub root: PathBuf,
    pub config: PathBuf,
    pub database: PathBuf,
    pub exports_dir: PathBuf,
    pub logs_dir: PathBuf,
}

impl ProjectPaths {
    pub fn new(root: impl AsRef<Path>, layout: &ProjectLayout) -> Self {
        let root = root.as_ref().to_path_buf();
        let config = root.join(&layout.config_filename);
        let database = root.join(&layout.database_filename);
        let exports_dir = root.join(&layout.exports_dirname);
        let logs_dir = root.join(&layout.logs_dirname);

        Self {
            root,
            config,
            database,
            exports_dir,
            logs_dir,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ProjectConfig, ProjectLayout, ProjectPaths};

    #[test]
    fn default_layout_uses_expected_names() {
        let layout = ProjectLayout::default();
        assert_eq!(layout.config_filename, "project.toml");
        assert_eq!(layout.database_filename, "crossfeed.db");
        assert_eq!(layout.exports_dirname, "exports");
        assert_eq!(layout.logs_dirname, "logs");
    }

    #[test]
    fn project_paths_join_layout_entries() {
        let layout = ProjectLayout::default();
        let paths = ProjectPaths::new("/tmp/crossfeed", &layout);

        assert_eq!(
            paths.config,
            std::path::Path::new("/tmp/crossfeed/project.toml")
        );
        assert_eq!(
            paths.database,
            std::path::Path::new("/tmp/crossfeed/crossfeed.db")
        );
        assert_eq!(
            paths.exports_dir,
            std::path::Path::new("/tmp/crossfeed/exports")
        );
        assert_eq!(paths.logs_dir, std::path::Path::new("/tmp/crossfeed/logs"));
    }

    #[test]
    fn project_config_defaults_include_body_limits() {
        let config = ProjectConfig::default();
        assert_eq!(config.timeline.body_limits_mb.request_max_mb, 40);
        assert_eq!(config.timeline.body_limits_mb.response_max_mb, 40);
    }

    #[test]
    fn project_config_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("project.toml");
        let mut config = ProjectConfig::default();
        config.timeline.body_limits_mb.request_max_mb = 64;
        config.timeline.body_limits_mb.response_max_mb = 128;
        config.proxy.listen_port = 9999;
        config.save(&path).unwrap();
        let loaded = ProjectConfig::load_or_create(&path).unwrap();
        assert_eq!(loaded, config);
    }
}
