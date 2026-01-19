use crossfeed_proxy::{ProxyEvent, ProxyEventKind};
use crossfeed_storage::{
    BodyLimits, TimelineEvent, TimelineStore, TimelineWorkerConfig, TimelineWorkerHandle,
    spawn_timeline_worker,
};

use futures::StreamExt;

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
