use std::collections::{HashMap, HashSet};

use tokio::sync::oneshot;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InterceptDecision<T> {
    Allow(T),
    Drop,
}

#[derive(Debug)]
pub enum InterceptResult<T> {
    Forward(T),
    Intercepted {
        id: Uuid,
        receiver: oneshot::Receiver<InterceptDecision<T>>,
    },
}

#[derive(Debug)]
pub struct InterceptManager<Request, Response> {
    request_intercept_enabled: bool,
    response_intercept_enabled: bool,
    pending_requests: HashMap<Uuid, Pending<Request>>,
    pending_responses: HashMap<Uuid, Pending<Response>>,
    response_intercept_for: HashSet<Uuid>,
}

#[derive(Debug)]
struct Pending<T> {
    value: T,
    sender: oneshot::Sender<InterceptDecision<T>>,
}

impl<Request: Clone, Response: Clone> Default for InterceptManager<Request, Response> {
    fn default() -> Self {
        Self {
            request_intercept_enabled: false,
            response_intercept_enabled: false,
            pending_requests: HashMap::new(),
            pending_responses: HashMap::new(),
            response_intercept_for: HashSet::new(),
        }
    }
}

impl<Request: Clone, Response: Clone> InterceptManager<Request, Response> {
    pub fn set_request_intercept(&mut self, enabled: bool) {
        if !enabled && self.request_intercept_enabled {
            let pending = std::mem::take(&mut self.pending_requests);
            for (id, pending) in pending {
                let _ = pending.sender.send(InterceptDecision::Allow(pending.value));
                self.response_intercept_for.remove(&id);
            }
        }
        self.request_intercept_enabled = enabled;
    }

    pub fn is_request_intercept_enabled(&self) -> bool {
        self.request_intercept_enabled
    }

    pub fn set_response_intercept(&mut self, enabled: bool) {
        if !enabled && self.response_intercept_enabled {
            let pending = std::mem::take(&mut self.pending_responses);
            for (_, pending) in pending {
                let _ = pending.sender.send(InterceptDecision::Allow(pending.value));
            }
        }
        self.response_intercept_enabled = enabled;
    }

    pub fn is_response_intercept_enabled(&self) -> bool {
        self.response_intercept_enabled
    }

    pub fn should_intercept_response_for_request(&self, request_id: Uuid) -> bool {
        self.response_intercept_for.contains(&request_id)
    }

    pub fn intercept_response_for_request(&mut self, request_id: Uuid) {
        self.response_intercept_for.insert(request_id);
    }

    pub fn intercept_request(&mut self, id: Uuid, request: Request) -> InterceptResult<Request> {
        if !self.request_intercept_enabled {
            return InterceptResult::Forward(request);
        }

        let (sender, receiver) = oneshot::channel();
        self.pending_requests.insert(
            id,
            Pending {
                value: request,
                sender,
            },
        );

        InterceptResult::Intercepted { id, receiver }
    }

    pub fn intercept_response(
        &mut self,
        request_id: Uuid,
        response_id: Uuid,
        response: Response,
    ) -> InterceptResult<Response> {
        let should_intercept =
            self.response_intercept_enabled || self.response_intercept_for.remove(&request_id);
        if !should_intercept {
            return InterceptResult::Forward(response);
        }

        let (sender, receiver) = oneshot::channel();
        self.pending_responses.insert(
            response_id,
            Pending {
                value: response,
                sender,
            },
        );

        InterceptResult::Intercepted {
            id: response_id,
            receiver,
        }
    }

    pub fn resolve_request(&mut self, id: Uuid, decision: InterceptDecision<Request>) -> bool {
        let Some(pending) = self.pending_requests.remove(&id) else {
            return false;
        };
        let _ = pending.sender.send(decision);
        true
    }

    pub fn resolve_response(&mut self, id: Uuid, decision: InterceptDecision<Response>) -> bool {
        let Some(pending) = self.pending_responses.remove(&id) else {
            return false;
        };
        let _ = pending.sender.send(decision);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::{InterceptDecision, InterceptManager, InterceptResult};

    #[tokio::test]
    async fn request_intercept_disabled_forwards() {
        let mut manager: InterceptManager<&str, &str> = InterceptManager::default();
        let id = uuid::Uuid::new_v4();
        let result = manager.intercept_request(id, "GET /index.html");
        assert!(matches!(
            result,
            InterceptResult::Forward("GET /index.html")
        ));
    }

    #[tokio::test]
    async fn request_intercept_enabled_waits_for_decision() {
        let mut manager: InterceptManager<&str, &str> = InterceptManager::default();
        manager.set_request_intercept(true);
        let request_id = uuid::Uuid::new_v4();
        let result = manager.intercept_request(request_id, "GET /index.html");
        let InterceptResult::Intercepted { id, receiver } = result else {
            panic!("expected intercepted request");
        };
        assert_eq!(id, request_id);
        assert!(manager.resolve_request(request_id, InterceptDecision::Allow("GET /edited")));
        let decision = receiver.await.expect("decision");
        assert_eq!(decision, InterceptDecision::Allow("GET /edited"));
    }

    #[tokio::test]
    async fn request_intercept_flushes_when_disabled() {
        let mut manager: InterceptManager<&str, &str> = InterceptManager::default();
        manager.set_request_intercept(true);
        let result = manager.intercept_request(uuid::Uuid::new_v4(), "GET /index.html");
        let InterceptResult::Intercepted { receiver, .. } = result else {
            panic!("expected intercepted request");
        };
        manager.set_request_intercept(false);
        let decision = receiver.await.expect("decision");
        assert_eq!(decision, InterceptDecision::Allow("GET /index.html"));
    }

    #[tokio::test]
    async fn response_intercept_for_request_overrides_toggle() {
        let mut manager: InterceptManager<&str, &str> = InterceptManager::default();
        let request_id = uuid::Uuid::new_v4();
        let response_id = uuid::Uuid::new_v4();
        manager.intercept_response_for_request(request_id);
        let result = manager.intercept_response(request_id, response_id, "HTTP/1.1 200 OK");
        let InterceptResult::Intercepted { receiver, id } = result else {
            panic!("expected intercepted response");
        };
        assert_eq!(id, response_id);
        manager.set_response_intercept(false);
        let decision = receiver.await.expect("decision");
        assert_eq!(decision, InterceptDecision::Allow("HTTP/1.1 200 OK"));
    }

    #[tokio::test]
    async fn response_intercept_disabled_forwards() {
        let mut manager: InterceptManager<&str, &str> = InterceptManager::default();
        let result = manager.intercept_response(
            uuid::Uuid::new_v4(),
            uuid::Uuid::new_v4(),
            "HTTP/1.1 200 OK",
        );
        assert!(matches!(
            result,
            InterceptResult::Forward("HTTP/1.1 200 OK")
        ));
    }
}
