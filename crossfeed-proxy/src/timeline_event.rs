use crossfeed_storage::{TimelineRequest, TimelineResponse};

#[derive(Debug, Clone)]
pub struct TimelineEvent {
    pub request: TimelineRequest,
    pub response: Option<TimelineResponse>,
}
