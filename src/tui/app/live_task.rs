//! Long-lived, watch-backed view data.
//!
//! [`super::async_task::AsyncTask`] models a one-shot fetch. A `LiveTask`
//! keeps a view's data current instead: the spawned task (see
//! [`crate::kube::live`]) pushes a new snapshot whenever the cluster state
//! behind the view changes, until the view is left and [`LiveTask::clear`]
//! aborts it. The request → dispatch → poll lifecycle matches `AsyncTask`, so
//! views and handlers use the same calls.

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, error::TryRecvError};

pub struct LiveTask<K, T> {
    /// Request queued by an event handler, waiting for the main loop.
    pending: Option<K>,
    /// The most recent request, kept while its task runs (see [`Self::key`]).
    key: Option<K>,
    /// Latest snapshot.
    result: Option<T>,
    /// Snapshots from the running task.
    rx: Option<UnboundedReceiver<anyhow::Result<T>>>,
    /// The running task, aborted on clear/drop.
    handle: Option<tokio::task::JoinHandle<()>>,
}

impl<K, T> Default for LiveTask<K, T> {
    fn default() -> Self {
        Self {
            pending: None,
            key: None,
            result: None,
            rx: None,
            handle: None,
        }
    }
}

impl<K: Clone, T> LiveTask<K, T> {
    /// Queue a new request, stopping any running task and dropping its data.
    pub fn request(&mut self, key: K) {
        self.clear();
        self.key = Some(key.clone());
        self.pending = Some(key);
    }

    /// The request currently queued or running, if any — lets callers avoid
    /// restarting a task that already serves the same request.
    pub fn key(&self) -> Option<&K> {
        self.key.as_ref()
    }

    /// Take the queued request and arm the channel the spawned task sends
    /// snapshots on. The caller must pass the task's handle to
    /// [`Self::set_handle`].
    pub fn dispatch(&mut self) -> Option<(K, UnboundedSender<anyhow::Result<T>>)> {
        let key = self.pending.take()?;
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        self.rx = Some(rx);
        Some((key, tx))
    }

    /// Store the spawned task's handle so clear() can abort it.
    pub fn set_handle(&mut self, handle: tokio::task::JoinHandle<()>) {
        self.handle = Some(handle);
    }

    /// The newest message from the task, draining older ones (only the
    /// latest snapshot matters). `None` when nothing new arrived.
    pub fn poll(&mut self) -> Option<anyhow::Result<T>> {
        let rx = self.rx.as_mut()?;
        let mut latest = None;
        loop {
            match rx.try_recv() {
                Ok(message) => latest = Some(message),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    // The task ended (initial fetch failed, or every watch
                    // was forbidden); keep the last snapshot.
                    self.rx = None;
                    break;
                }
            }
        }
        latest
    }

    /// Store a snapshot.
    pub fn set_result(&mut self, value: T) {
        self.result = Some(value);
    }

    /// The latest snapshot, if any.
    pub fn result(&self) -> Option<&T> {
        self.result.as_ref()
    }

    /// The queued (not yet dispatched) request key, if any.
    pub fn pending(&self) -> Option<&K> {
        self.pending.as_ref()
    }

    /// Whether the first snapshot is still on its way.
    pub fn is_loading(&self) -> bool {
        self.result.is_none() && (self.pending.is_some() || self.rx.is_some())
    }

    /// Stop the task and drop all state.
    pub fn clear(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
        self.pending = None;
        self.key = None;
        self.result = None;
        self.rx = None;
    }
}

impl<K, T> Drop for LiveTask<K, T> {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
    }
}

impl<K: std::fmt::Debug, T> std::fmt::Debug for LiveTask<K, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LiveTask")
            .field("pending", &self.pending)
            .field("has_result", &self.result.is_some())
            .field("running", &self.handle.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_keeps_latest_snapshot() {
        let mut task: LiveTask<String, u32> = LiveTask::default();
        task.request("key".to_string());
        assert!(task.is_loading());

        let (key, tx) = task.dispatch().unwrap();
        assert_eq!(key, "key");
        assert!(task.is_loading(), "still loading until the first snapshot");

        tx.send(Ok(1)).unwrap();
        tx.send(Ok(2)).unwrap();
        let latest = task.poll().unwrap().unwrap();
        assert_eq!(latest, 2, "older snapshots are superseded");
        task.set_result(latest);
        assert!(!task.is_loading());
        assert!(task.poll().is_none());

        // A later snapshot replaces the stored one.
        tx.send(Ok(3)).unwrap();
        let next = task.poll().unwrap().unwrap();
        task.set_result(next);
        assert_eq!(task.result(), Some(&3));
    }

    #[test]
    fn ended_task_keeps_last_snapshot() {
        let mut task: LiveTask<String, u32> = LiveTask::default();
        task.request("key".to_string());
        let (_, tx) = task.dispatch().unwrap();
        tx.send(Ok(7)).unwrap();
        drop(tx);
        let next = task.poll().unwrap().unwrap();
        task.set_result(next);
        assert!(task.poll().is_none());
        assert_eq!(task.result(), Some(&7));
        assert!(!task.is_loading());
    }

    #[test]
    fn request_replaces_previous_state() {
        let mut task: LiveTask<String, u32> = LiveTask::default();
        task.set_result(1);
        task.request("next".to_string());
        assert!(task.result().is_none());
        assert_eq!(task.pending().map(String::as_str), Some("next"));
    }
}
