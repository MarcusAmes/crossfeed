use clap::Parser;
use std::path::{Path, PathBuf};

use crossfeed_ingest::IngestHandle;
use crossfeed_proxy::{Proxy, ProxyConfig};
use crossfeed_storage::{ProjectLayout, ProjectPaths, SqliteStore};

#[derive(Debug, Parser)]
#[command(name = "crossfeed-proxy-cli")]
struct Cli {
    #[arg(long = "proxy-dir")]
    proxy_dir: PathBuf,
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
    let ingest = IngestHandle::new(Box::new(store), crossfeed_storage::BodyLimits::default());

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
