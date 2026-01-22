mod project_runtime;
mod proxy_runtime;
mod replay_runtime;
mod timeline_tail;

use crossfeed_proxy::{ProxyEvent, ProxyEventKind};
use crossfeed_storage::{
    BodyLimits, TimelineEvent, TimelineStore, TimelineWorkerConfig, TimelineWorkerHandle,
    spawn_timeline_worker,
};

use futures::StreamExt;

pub use project_runtime::{ProjectContext, open_or_create_project};
pub use proxy_runtime::{ProxyRuntimeConfig, start_proxy};
pub use replay_runtime::{
    create_collection_and_add_request, create_replay_collection, create_replay_from_timeline,
    duplicate_replay_request, get_latest_replay_execution, get_latest_replay_response,
    get_replay_active_version, get_replay_request, list_replay_collections,
    list_replay_requests_in_collection, list_replay_requests_unassigned,
    move_replay_request_to_collection, update_replay_collection_color,
    update_replay_collection_name, update_replay_collection_sort,
    update_replay_request_name, update_replay_request_sort,
};
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
}

impl IngestHandle {
    pub fn new(store: Box<dyn TimelineStore>, limits: BodyLimits) -> Self {
        let worker = spawn_timeline_worker(store, limits, TimelineWorkerConfig::default());
        Self { worker }
    }

    pub fn from_worker(worker: TimelineWorkerHandle) -> Self {
        Self { worker }
    }

    pub async fn ingest_stream(&self, mut events: impl futures::Stream<Item = ProxyEvent> + Unpin) {
        while let Some(event) = events.next().await {
            if let Some(timeline) = map_proxy_event(event) {
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
