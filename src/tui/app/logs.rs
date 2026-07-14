//! Controller pod log streaming state.
//!
//! The log view follows the app's non-blocking pattern: a handler queues a
//! [`LogRequest`], the main loop dispatches it (spawning the kube log stream
//! task), and each tick drains streamed lines into a bounded buffer. The
//! stream task runs only while the log view is open — leaving the view stops
//! it, mirroring the events watcher lifecycle.

use std::collections::VecDeque;

/// Which pod to stream logs from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogRequest {
    pub namespace: String,
    pub pod: String,
}

/// Message from the log stream task to the app.
#[derive(Debug)]
pub enum LogEvent {
    /// One log line.
    Line(String),
    /// The stream failed (RBAC, pod gone, container restart, …).
    Error(String),
    /// The stream ended cleanly (pod terminated).
    Ended,
}

/// A running log stream: its bounded line buffer and the channel/handle of
/// the streaming task.
#[derive(Debug)]
pub struct LogSession {
    pub pod: String,
    pub namespace: String,
    lines: VecDeque<String>,
    rx: tokio::sync::mpsc::UnboundedReceiver<LogEvent>,
    /// Handle of the streaming task; set by the main loop right after
    /// dispatch+spawn. Aborted on stop.
    handle: Option<tokio::task::JoinHandle<()>>,
    /// Set when the stream ended or failed; shown in the view title.
    pub status: Option<String>,
}

impl LogSession {
    /// Drain any streamed lines into the buffer, evicting the oldest past
    /// [`crate::constants::MAX_LOG_LINES`]. Returns how many arrived.
    fn drain(&mut self) -> usize {
        let mut received = 0;
        while let Ok(event) = self.rx.try_recv() {
            match event {
                LogEvent::Line(line) => {
                    self.lines.push_back(line);
                    if self.lines.len() > crate::constants::MAX_LOG_LINES {
                        self.lines.pop_front();
                    }
                    received += 1;
                }
                LogEvent::Error(e) => self.status = Some(format!("stream error: {}", e)),
                LogEvent::Ended => self.status = Some("stream ended".to_string()),
            }
        }
        received
    }

    pub fn lines(&self) -> &VecDeque<String> {
        &self.lines
    }
}

/// Log view state: at most one queued request and one active session.
#[derive(Debug, Default)]
pub struct LogState {
    /// Request waiting for the main loop to dispatch.
    pending: Option<LogRequest>,
    /// The active stream, if any.
    pub session: Option<LogSession>,
    /// Auto-scroll to the newest line. Scrolling up pauses following;
    /// `G` jumps to the bottom and resumes.
    pub follow: bool,
}

impl LogState {
    /// Queue a log stream for the given pod, replacing (and stopping) any
    /// active session.
    pub fn request(&mut self, namespace: String, pod: String) {
        self.stop();
        self.pending = Some(LogRequest { namespace, pod });
        self.follow = true;
    }

    /// Take the queued request and register the session the main loop spawns.
    /// Returns the request and the sender for the stream task; the caller
    /// must pass the spawned task's handle to [`Self::set_handle`].
    pub fn dispatch(
        &mut self,
    ) -> Option<(LogRequest, tokio::sync::mpsc::UnboundedSender<LogEvent>)> {
        let request = self.pending.take()?;
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        self.session = Some(LogSession {
            pod: request.pod.clone(),
            namespace: request.namespace.clone(),
            lines: VecDeque::new(),
            rx,
            handle: None,
            status: None,
        });
        Some((request, tx))
    }

    /// Store the stream-task handle after spawning so stop() can abort it.
    pub fn set_handle(&mut self, handle: tokio::task::JoinHandle<()>) {
        if let Some(ref mut session) = self.session {
            session.handle = Some(handle);
        }
    }

    /// Drain streamed lines into the buffer. Returns how many arrived.
    pub fn drain(&mut self) -> usize {
        self.session.as_mut().map_or(0, LogSession::drain)
    }

    /// Whether a request is queued or a stream is running.
    pub fn is_loading(&self) -> bool {
        self.pending.is_some()
            || self
                .session
                .as_ref()
                .is_some_and(|session| session.status.is_none())
    }

    /// Stop the stream and drop the session (called when leaving the view).
    pub fn stop(&mut self) {
        self.pending = None;
        if let Some(session) = self.session.take() {
            if let Some(handle) = session.handle {
                handle.abort();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn request_dispatch_drain_roundtrip() {
        let mut state = LogState::default();
        assert!(state.dispatch().is_none());

        state.request(
            "flux-system".to_string(),
            "source-controller-abc".to_string(),
        );
        assert!(state.is_loading());
        assert!(state.follow);

        let (request, tx) = state.dispatch().expect("queued request dispatches");
        assert_eq!(request.pod, "source-controller-abc");
        assert_eq!(request.namespace, "flux-system");

        tx.send(LogEvent::Line("line 1".to_string())).unwrap();
        tx.send(LogEvent::Line("line 2".to_string())).unwrap();
        assert_eq!(state.drain(), 2);
        let session = state.session.as_ref().unwrap();
        assert_eq!(session.lines().len(), 2);
        assert!(state.is_loading(), "stream still running");

        tx.send(LogEvent::Ended).unwrap();
        state.drain();
        assert!(!state.is_loading(), "ended stream is no longer loading");
        assert!(
            state
                .session
                .as_ref()
                .unwrap()
                .status
                .as_deref()
                .unwrap()
                .contains("ended")
        );
    }

    #[tokio::test]
    async fn buffer_is_bounded() {
        let mut state = LogState::default();
        state.request("ns".to_string(), "pod".to_string());
        let (_, tx) = state.dispatch().unwrap();
        for i in 0..(crate::constants::MAX_LOG_LINES + 10) {
            tx.send(LogEvent::Line(format!("line {i}"))).unwrap();
        }
        state.drain();
        let session = state.session.as_ref().unwrap();
        assert_eq!(session.lines().len(), crate::constants::MAX_LOG_LINES);
        assert_eq!(
            session.lines().front().unwrap(),
            "line 10",
            "oldest lines evicted first"
        );
    }

    #[tokio::test]
    async fn new_request_replaces_session_and_stop_clears() {
        let mut state = LogState::default();
        state.request("ns".to_string(), "pod-a".to_string());
        let _ = state.dispatch();
        assert!(state.session.is_some());

        state.request("ns".to_string(), "pod-b".to_string());
        assert!(
            state.session.is_none(),
            "old session stopped on new request"
        );
        let _ = state.dispatch();
        assert_eq!(state.session.as_ref().unwrap().pod, "pod-b");

        state.stop();
        assert!(state.session.is_none());
        assert!(!state.is_loading());
    }

    #[tokio::test]
    async fn stream_error_is_surfaced() {
        let mut state = LogState::default();
        state.request("ns".to_string(), "pod".to_string());
        let (_, tx) = state.dispatch().unwrap();
        tx.send(LogEvent::Error("forbidden".to_string())).unwrap();
        state.drain();
        assert!(
            state
                .session
                .as_ref()
                .unwrap()
                .status
                .as_deref()
                .unwrap()
                .contains("forbidden")
        );
    }
}
