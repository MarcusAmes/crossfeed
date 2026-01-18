use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::timeline_event::TimelineEvent;

pub type ProxyEvents = ReceiverStream<TimelineEvent>;

pub fn event_channel() -> (mpsc::Sender<TimelineEvent>, ProxyEvents) {
    let (sender, receiver) = mpsc::channel(50_000);
    (sender, ReceiverStream::new(receiver))
}
