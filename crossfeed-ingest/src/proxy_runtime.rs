use std::path::PathBuf;

use crossfeed_net::load_or_generate_ca;
use crossfeed_proxy::{Proxy, ProxyConfig, ProxyEvents};
use crossfeed_storage::{BodyLimits, SqliteStore};

use crate::{IngestHandle, ProjectContext};

#[derive(Debug, Clone)]
pub struct ProxyRuntimeConfig {
    pub certs_dir: PathBuf,
    pub leaf_dir: PathBuf,
    pub listen_host: String,
    pub listen_port: u16,
    pub body_limits: BodyLimits,
}

impl ProxyRuntimeConfig {
    pub fn from_project(context: &ProjectContext, certs_dir: PathBuf) -> Self {
        let leaf_dir = certs_dir.join("leaf");
        let body_limits = BodyLimits {
            request_max_bytes: context.config.timeline.body_limits_mb.request_max_mb as usize
                * 1024
                * 1024,
            response_max_bytes: context.config.timeline.body_limits_mb.response_max_mb as usize
                * 1024
                * 1024,
        };
        Self {
            certs_dir,
            leaf_dir,
            listen_host: context.config.proxy.listen_host.clone(),
            listen_port: context.config.proxy.listen_port,
            body_limits,
        }
    }
}

pub async fn start_proxy(
    context: ProjectContext,
    config: ProxyRuntimeConfig,
) -> Result<(), String> {
    std::fs::create_dir_all(&config.certs_dir).map_err(|err| err.to_string())?;
    std::fs::create_dir_all(&config.leaf_dir).map_err(|err| err.to_string())?;

    let store = SqliteStore::open(&context.store_path)?;
    let ingest = IngestHandle::new(Box::new(store), config.body_limits);

    let mut proxy_config = ProxyConfig::default();
    proxy_config.listen.host = config.listen_host;
    proxy_config.listen.port = config.listen_port;
    proxy_config.tls.ca_cert_dir = config.certs_dir.to_string_lossy().into_owned();
    proxy_config.tls.leaf_cert_dir = config.leaf_dir.to_string_lossy().into_owned();

    let _ = load_or_generate_ca(
        &proxy_config.tls.ca_cert_dir,
        &proxy_config.tls.ca_common_name,
    )
    .map_err(|err| err.message)?;

    let (proxy, events, _control) = Proxy::new(proxy_config).map_err(|err| err.to_string())?;
    run_proxy(proxy, events, ingest).await
}

async fn run_proxy(proxy: Proxy, events: ProxyEvents, ingest: IngestHandle) -> Result<(), String> {
    let ingest_task = tokio::spawn(async move {
        ingest.ingest_stream(events).await;
    });
    let proxy_task = tokio::spawn(async move { proxy.run().await });
    let _ = tokio::try_join!(proxy_task, ingest_task);
    Ok(())
}

#[cfg(feature = "sync-runtime")]
pub fn start_proxy_sync(context: ProjectContext, config: ProxyRuntimeConfig) -> Result<(), String> {
    let runtime = tokio::runtime::Runtime::new().map_err(|err| err.to_string())?;
    runtime.block_on(start_proxy(context, config))
}
