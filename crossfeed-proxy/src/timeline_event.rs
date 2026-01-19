use crossfeed_storage::{TimelineRequest, TimelineResponse};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProxyRequest {
    pub id: Uuid,
    pub timeline: TimelineRequest,
    pub raw_request: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProxyResponse {
    pub id: Uuid,
    pub timeline: TimelineResponse,
    pub raw_response: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ProxyEventKind {
    RequestObserved,
    RequestIntercepted,
    RequestForwarded,
    ResponseObserved,
    ResponseIntercepted,
    ResponseForwarded,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProxyEvent {
    pub event_id: Uuid,
    pub request_id: Uuid,
    pub kind: ProxyEventKind,
    pub request: Option<ProxyRequest>,
    pub response: Option<ProxyResponse>,
}
