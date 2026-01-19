use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

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
            database_filename: "db.sqlite".to_string(),
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
    use super::{ProjectLayout, ProjectPaths};

    #[test]
    fn default_layout_uses_expected_names() {
        let layout = ProjectLayout::default();
        assert_eq!(layout.config_filename, "project.toml");
        assert_eq!(layout.database_filename, "db.sqlite");
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
            std::path::Path::new("/tmp/crossfeed/db.sqlite")
        );
        assert_eq!(
            paths.exports_dir,
            std::path::Path::new("/tmp/crossfeed/exports")
        );
        assert_eq!(paths.logs_dir, std::path::Path::new("/tmp/crossfeed/logs"));
    }
}
