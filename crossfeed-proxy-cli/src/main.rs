use clap::Parser;
use std::path::{Path, PathBuf};

use crossfeed_ingest::IngestHandle;
use crossfeed_proxy::{Proxy, ProxyConfig};
use crossfeed_storage::{ProjectConfig, ProjectLayout, ProjectPaths, SqliteStore};

#[derive(Debug, Parser)]
#[command(name = "crossfeed-proxy-cli")]
struct Cli {
    #[arg(long = "proxy-dir")]
    proxy_dir: PathBuf,
    #[arg(long = "request-body-limit-mb", default_value_t = 40)]
    request_body_limit_mb: usize,
    #[arg(long = "response-body-limit-mb", default_value_t = 40)]
    response_body_limit_mb: usize,
}

#[tokio::main]
async fn main() -> Result<(), String> {
    let cli = Cli::parse();
    let layout = ProjectLayout::default();
    let paths = ProjectPaths::new(&cli.proxy_dir, &layout);

    ensure_dir(&paths.root)?;
    ensure_dir(&paths.exports_dir)?;
    ensure_dir(&paths.logs_dir)?;

    let certs_dir = paths.root.join("certs");
    let leaf_dir = certs_dir.join("leaf");
    ensure_dir(&certs_dir)?;
    ensure_dir(&leaf_dir)?;

    let store = SqliteStore::open(&paths.database)?;
    let config = ProjectConfig::load_or_create(&paths.config)?;
    let default_request_mb = config.timeline.body_limits_mb.request_max_mb as usize;
    let default_response_mb = config.timeline.body_limits_mb.response_max_mb as usize;
    let limits = crossfeed_storage::BodyLimits {
        request_max_bytes: cli.request_body_limit_mb.max(default_request_mb) * 1024 * 1024,
        response_max_bytes: cli.response_body_limit_mb.max(default_response_mb) * 1024 * 1024,
    };
    let ingest = IngestHandle::new(Box::new(store), limits);

    let mut proxy_config = ProxyConfig::default();
    proxy_config.tls.ca_cert_dir = certs_dir.to_string_lossy().into_owned();
    proxy_config.tls.leaf_cert_dir = leaf_dir.to_string_lossy().into_owned();

    let (proxy, events, _control) = Proxy::new(proxy_config).map_err(|err| err.to_string())?;

    let ingest_task = tokio::spawn(async move {
        ingest.ingest_stream(events).await;
    });

    let proxy_task = tokio::spawn(async move { proxy.run().await });

    let _ = tokio::try_join!(proxy_task, ingest_task);

    Ok(())
}

fn ensure_dir(path: &Path) -> Result<(), String> {
    std::fs::create_dir_all(path).map_err(|err| err.to_string())
}
