use std::path::{Path, PathBuf};

use crossfeed_storage::{ProjectConfig, ProjectLayout, ProjectPaths, SqliteStore};

#[derive(Debug, Clone)]
pub struct ProjectContext {
    pub paths: ProjectPaths,
    pub config: ProjectConfig,
    pub store_path: PathBuf,
}

pub fn open_or_create_project(path: impl AsRef<Path>) -> Result<ProjectContext, String> {
    let layout = ProjectLayout::default();
    let paths = ProjectPaths::new(path.as_ref(), &layout);
    ensure_dir(&paths.root)?;
    ensure_dir(&paths.exports_dir)?;
    ensure_dir(&paths.logs_dir)?;
    let config = ProjectConfig::load_or_create(&paths.config)?;
    SqliteStore::open(&paths.database)?;
    Ok(ProjectContext {
        paths: paths.clone(),
        config,
        store_path: paths.database.clone(),
    })
}

fn ensure_dir(path: &Path) -> Result<(), String> {
    std::fs::create_dir_all(path).map_err(|err| err.to_string())
}

#[cfg(feature = "sync-runtime")]
pub fn open_or_create_project_sync(path: impl AsRef<Path>) -> Result<ProjectContext, String> {
    open_or_create_project(path)
}
