mod project_runtime;
mod proxy_runtime;
mod replay_runtime;
mod scope;
mod timeline_tail;

use crossfeed_proxy::{ProxyEvent, ProxyEventKind};
use crossfeed_storage::{
    BodyLimits, TimelineEvent, TimelineStore, TimelineWorkerConfig, TimelineWorkerHandle,
    spawn_timeline_worker,
};
use std::path::PathBuf;

use futures::StreamExt;

pub use project_runtime::{ProjectContext, open_or_create_project};
pub use proxy_runtime::{ProxyRuntimeConfig, start_proxy};
pub use replay_runtime::{
    activate_latest_replay_child, apply_replay_edit, apply_replay_raw_edit,
    create_collection_and_add_request, create_replay_collection, create_replay_from_timeline,
    duplicate_replay_request,
    get_latest_replay_execution, get_latest_replay_response, get_replay_active_version,
    get_replay_request, list_replay_collections, list_replay_requests_in_collection,
    list_replay_requests_unassigned, move_replay_request_to_collection,
    send_replay_request, set_replay_active_version, update_replay_collection_color,
    update_replay_collection_name, update_replay_collection_sort, update_replay_request_name,
    update_replay_request_sort,
};
pub use crossfeed_web::CancelToken;
pub use crossfeed_replay::ReplayEdit;
pub use scope::{ScopeEvaluation, evaluate_scope};
pub use timeline_tail::{TailCursor, TailUpdate, TimelineItem, tail_query};

#[cfg(feature = "sync-runtime")]
pub use project_runtime::open_or_create_project_sync;
#[cfg(feature = "sync-runtime")]
pub use proxy_runtime::start_proxy_sync;
#[cfg(feature = "sync-runtime")]
pub use timeline_tail::tail_query_sync;

#[derive(Debug, Clone)]
pub struct IngestHandle {
    worker: TimelineWorkerHandle,
    store_path: PathBuf,
}

impl IngestHandle {
    pub fn new(store: Box<dyn TimelineStore>, limits: BodyLimits) -> Self {
        let worker = spawn_timeline_worker(store, limits, TimelineWorkerConfig::default());
        Self {
            worker,
            store_path: PathBuf::new(),
        }
    }

    pub fn new_with_path(
        store_path: PathBuf,
        store: Box<dyn TimelineStore>,
        limits: BodyLimits,
    ) -> Self {
        let worker = spawn_timeline_worker(store, limits, TimelineWorkerConfig::default());
        Self { worker, store_path }
    }

    pub fn from_worker(worker: TimelineWorkerHandle) -> Self {
        Self {
            worker,
            store_path: PathBuf::new(),
        }
    }

    pub async fn ingest_stream(&self, mut events: impl futures::Stream<Item = ProxyEvent> + Unpin) {
        while let Some(event) = events.next().await {
            if let Some(mut timeline) = map_proxy_event(event) {
                if !self.store_path.as_os_str().is_empty() {
                    if let Ok(scope) = evaluate_scope(
                        &self.store_path,
                        &timeline.request.host,
                        &timeline.request.path,
                    ) {
                        timeline.request.scope_status_at_capture = scope.scope_status_at_capture;
                        timeline.request.scope_rules_version = scope.scope_rules_version;
                        timeline.request.capture_filtered = scope.capture_filtered;
                        timeline.request.timeline_filtered = scope.timeline_filtered;
                    }
                }
                let _ = self.worker.send(timeline);
            }
        }
    }
}

fn map_proxy_event(event: ProxyEvent) -> Option<TimelineEvent> {
    match event.kind {
        ProxyEventKind::ResponseForwarded => {
            let request = event.request?;
            let response = event.response?;
            Some(TimelineEvent {
                request: request.timeline,
                response: Some(response.timeline),
            })
        }
        _ => None,
    }
}
