use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, bounded};

use crate::timeline::{
    BodyLimits, TimelineInsertResult, TimelineRecorder, TimelineRequest, TimelineResponse,
    TimelineStore,
};

#[derive(Debug, Clone)]
pub struct TimelineWorkerConfig {
    pub batch_size: usize,
    pub flush_interval_ms: u64,
    pub max_queue_size: usize,
}

impl Default for TimelineWorkerConfig {
    fn default() -> Self {
        Self {
            batch_size: 500,
            flush_interval_ms: 200,
            max_queue_size: 50_000,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TimelineWorkerHandle {
    sender: Sender<TimelineEvent>,
}

#[derive(Debug, Clone)]
pub struct TimelineEvent {
    pub request: TimelineRequest,
    pub response: Option<TimelineResponse>,
}

impl TimelineWorkerHandle {
    pub fn send(&self, event: TimelineEvent) -> Result<(), String> {
        self.sender.send(event).map_err(|err| err.to_string())
    }
}

pub fn spawn_timeline_worker(
    store: Box<dyn TimelineStore>,
    limits: BodyLimits,
    config: TimelineWorkerConfig,
) -> TimelineWorkerHandle {
    let (sender, receiver) = bounded(config.max_queue_size);
    let recorder = TimelineRecorder::new(store, limits);

    std::thread::spawn(move || worker_loop(receiver, recorder, config));

    TimelineWorkerHandle { sender }
}

fn worker_loop(
    receiver: Receiver<TimelineEvent>,
    recorder: TimelineRecorder,
    config: TimelineWorkerConfig,
) {
    let mut batch = Vec::with_capacity(config.batch_size);
    let mut last_flush = Instant::now();

    loop {
        let timeout = Duration::from_millis(config.flush_interval_ms);
        match receiver.recv_timeout(timeout) {
            Ok(event) => {
                batch.push(event);
                if batch.len() >= config.batch_size {
                    flush_batch(&recorder, &mut batch);
                    last_flush = Instant::now();
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                if !batch.is_empty() && last_flush.elapsed() >= timeout {
                    flush_batch(&recorder, &mut batch);
                    last_flush = Instant::now();
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
        }
    }
}

fn flush_batch(recorder: &TimelineRecorder, batch: &mut Vec<TimelineEvent>) {
    for event in batch.drain(..) {
        if let Ok(TimelineInsertResult { request_id }) = recorder.record_request(event.request) {
            if let Some(mut response) = event.response {
                response.timeline_request_id = request_id;
                let _ = recorder.record_response(response);
            }
        }
    }
}
