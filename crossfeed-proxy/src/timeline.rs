use crossfeed_storage::{BodyLimits, TimelineRecorder, TimelineRequest, TimelineResponse, TimelineStore};
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub struct TimelineEvent {
    pub request: TimelineRequest,
    pub response: Option<TimelineResponse>,
}

#[derive(Debug, Clone)]
pub struct TimelineSink {
    sender: mpsc::Sender<TimelineEvent>,
}

impl TimelineSink {
    pub fn new(sender: mpsc::Sender<TimelineEvent>) -> Self {
        Self { sender }
    }

    pub async fn send(&self, event: TimelineEvent) -> Result<(), String> {
        self.sender.send(event).await.map_err(|err| err.to_string())
    }
}

pub fn spawn_timeline_worker(
    store: std::sync::Arc<dyn TimelineStore>,
    limits: BodyLimits,
) -> TimelineSink {
    let (sender, mut receiver) = mpsc::channel::<TimelineEvent>(128);
    let recorder = TimelineRecorder::new(store, limits);

    tokio::spawn(async move {
        while let Some(event) = receiver.recv().await {
            if let Ok(inserted) = recorder.record_request(event.request) {
                if let Some(mut response) = event.response {
                    response.timeline_request_id = inserted.request_id;
                    let _ = recorder.record_response(response);
                }
            }
        }
    });

    TimelineSink::new(sender)
}
