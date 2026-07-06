//! Generic single-slot async fetch task.
//!
//! Every "fetch something for a view" flow (YAML, describe, trace, graph, …)
//! follows the same three-phase lifecycle:
//!
//! 1. An event handler calls [`AsyncTask::request`] with the request key.
//! 2. The main loop calls [`AsyncTask::dispatch`], spawns a task that does the
//!    work, and sends the outcome through the returned sender.
//! 3. The main loop polls [`AsyncTask::try_recv`] each tick and stores the
//!    outcome with [`AsyncTask::set_result`] / [`AsyncTask::set_error`].
//!
//! `AsyncTask` owns that lifecycle once, instead of each feature carrying its
//! own `*_pending` / `*_fetched` / `*_rx` field triplet and hand-rolled
//! trigger/poll methods.

/// A single in-flight async fetch: the queued request key, the latest result,
/// and the channel for the running task. `K` identifies what was requested
/// (typically a [`crate::watcher::ResourceKey`]); `T` is the fetched payload.
pub struct AsyncTask<K, T> {
    /// Request queued by an event handler, waiting for the main loop to dispatch.
    pending: Option<K>,
    /// Latest successful result (cleared when a new request is queued).
    result: Option<T>,
    /// Receiver for the currently running task, if any.
    rx: Option<tokio::sync::oneshot::Receiver<anyhow::Result<T>>>,
}

/// Manual impl: the derived `Default` would require `K: Default + T: Default`
/// even though all fields are `Option`s.
impl<K, T> Default for AsyncTask<K, T> {
    fn default() -> Self {
        Self {
            pending: None,
            result: None,
            rx: None,
        }
    }
}

impl<K, T> AsyncTask<K, T> {
    /// Queue a new request, discarding any previous result so views show the
    /// loading state instead of stale data.
    pub fn request(&mut self, key: K) {
        self.pending = Some(key);
        self.result = None;
    }

    /// Take the queued request and arm the result channel.
    ///
    /// Returns the request key and the sender the spawned task must complete
    /// with. Returns `None` when nothing is queued.
    pub fn dispatch(&mut self) -> Option<(K, tokio::sync::oneshot::Sender<anyhow::Result<T>>)> {
        let key = self.pending.take()?;
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.rx = Some(rx);
        Some((key, tx))
    }

    /// Non-blocking poll for the running task's outcome.
    ///
    /// Returns the outcome exactly once; a dropped sender is reported as an
    /// error. Returns `None` while the task is still running or none is.
    pub fn try_recv(&mut self) -> Option<anyhow::Result<T>> {
        let rx = self.rx.as_mut()?;
        match rx.try_recv() {
            Ok(result) => {
                self.rx = None;
                Some(result)
            }
            Err(tokio::sync::oneshot::error::TryRecvError::Empty) => None,
            Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                self.rx = None;
                Some(Err(anyhow::anyhow!("async task dropped without a result")))
            }
        }
    }

    /// Store a successful result.
    pub fn set_result(&mut self, value: T) {
        self.result = Some(value);
    }

    /// Record a failure: drop any partial state so views fall back to their
    /// empty/error rendering instead of a stale result.
    pub fn set_error(&mut self) {
        self.pending = None;
        self.result = None;
    }

    /// The latest stored result, if any.
    pub fn result(&self) -> Option<&T> {
        self.result.as_ref()
    }

    /// The queued (not yet dispatched) request key, if any.
    pub fn pending(&self) -> Option<&K> {
        self.pending.as_ref()
    }

    /// Whether a request is queued or a task is currently running — i.e. the
    /// view should render a loading state rather than "no data".
    pub fn is_loading(&self) -> bool {
        self.pending.is_some() || self.rx.is_some()
    }

    /// Reset to the idle state, dropping any queued request, running task
    /// channel, and stored result.
    pub fn clear(&mut self) {
        self.pending = None;
        self.result = None;
        self.rx = None;
    }
}

/// Summarize lifecycle state without requiring `T: Debug` (results can be
/// large fetched objects) — pending key, whether a result is stored, and
/// whether a task is in flight.
impl<K: std::fmt::Debug, T> std::fmt::Debug for AsyncTask<K, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AsyncTask")
            .field("pending", &self.pending)
            .field("has_result", &self.result.is_some())
            .field("in_flight", &self.rx.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_dispatch_recv_roundtrip() {
        let mut task: AsyncTask<String, u32> = AsyncTask::default();
        assert!(!task.is_loading());
        assert!(task.dispatch().is_none());

        task.request("key".to_string());
        assert_eq!(task.pending(), Some(&"key".to_string()));
        assert!(task.is_loading());

        let (key, tx) = task.dispatch().expect("queued request should dispatch");
        assert_eq!(key, "key");
        assert!(task.pending().is_none());
        assert!(task.is_loading(), "in-flight task still counts as loading");

        assert!(task.try_recv().is_none(), "no result sent yet");
        tx.send(Ok(42)).unwrap();
        let result = task.try_recv().expect("result should arrive");
        assert_eq!(result.unwrap(), 42);
        assert!(
            task.try_recv().is_none(),
            "result is delivered exactly once"
        );
        assert!(!task.is_loading());

        task.set_result(42);
        assert_eq!(task.result(), Some(&42));
    }

    #[test]
    fn request_clears_previous_result() {
        let mut task: AsyncTask<String, u32> = AsyncTask::default();
        task.set_result(1);
        task.request("next".to_string());
        assert!(task.result().is_none());
    }

    #[test]
    fn dropped_sender_reports_error() {
        let mut task: AsyncTask<String, u32> = AsyncTask::default();
        task.request("key".to_string());
        let (_, tx) = task.dispatch().unwrap();
        drop(tx);
        let result = task.try_recv().expect("closed channel should surface");
        assert!(result.is_err());
        assert!(!task.is_loading());
    }

    #[test]
    fn set_error_and_clear_reset_state() {
        let mut task: AsyncTask<String, u32> = AsyncTask::default();
        task.request("key".to_string());
        task.set_error();
        assert!(task.pending().is_none());
        assert!(task.result().is_none());

        task.request("key2".to_string());
        let _ = task.dispatch();
        task.clear();
        assert!(!task.is_loading());
        assert!(task.try_recv().is_none());
    }
}
