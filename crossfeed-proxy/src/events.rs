use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::timeline_event::ProxyEvent;

pub type ProxyEvents = ReceiverStream<ProxyEvent>;

#[derive(Debug, Clone)]
pub struct ProxyControl {
    pub sender: mpsc::Sender<ProxyCommand>,
}

#[derive(Debug, Clone)]
pub enum ProxyCommand {
    SetRequestIntercept(bool),
    SetResponseIntercept(bool),
    InterceptResponseForRequest(uuid::Uuid),
    DecideRequest {
        id: uuid::Uuid,
        decision: crate::intercept::InterceptDecision<crate::timeline_event::ProxyRequest>,
    },
    DecideResponse {
        id: uuid::Uuid,
        decision: crate::intercept::InterceptDecision<crate::timeline_event::ProxyResponse>,
    },
}

pub fn event_channel() -> (mpsc::Sender<ProxyEvent>, ProxyEvents) {
    let (sender, receiver) = mpsc::channel(50_000);
    (sender, ReceiverStream::new(receiver))
}

pub fn control_channel() -> (ProxyControl, mpsc::Receiver<ProxyCommand>) {
    let (sender, receiver) = mpsc::channel(10_000);
    (ProxyControl { sender }, receiver)
}
