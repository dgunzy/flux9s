//! The sweep behind the ownership list (#274): an [`AsyncTask`] plus the
//! running sweep's progress, so the list can say how far a slow sweep has
//! got instead of a static "please wait".

use super::async_task::AsyncTask;
use crate::kube::sweep::{SweepPhase, SweepProgress, SweepRequest, SweepResult};
use std::time::Instant;
use tokio::sync::{oneshot, watch};

/// What the main loop needs to run a dispatched sweep: the request, where
/// its result goes, and where it reports progress.
pub struct SweepDispatch {
    pub request: SweepRequest,
    pub result: oneshot::Sender<anyhow::Result<SweepResult>>,
    pub progress: watch::Sender<SweepPhase>,
}

/// One sweep at a time: queued, running (with progress), or finished.
#[derive(Debug, Default)]
pub struct SweepTask {
    task: AsyncTask<SweepRequest, SweepResult>,
    /// The running sweep's phase, and when it started.
    progress: Option<(watch::Receiver<SweepPhase>, Instant)>,
}

impl SweepTask {
    /// Queue a sweep, dropping the rows shown so far.
    pub fn request(&mut self, request: SweepRequest) {
        self.task.request(request);
        self.progress = None;
    }

    /// Queue a sweep, keeping the rows shown so far until its rows arrive
    /// (`Ctrl-r`).
    pub fn refresh(&mut self, request: SweepRequest) {
        self.task.refresh(request);
        self.progress = None;
    }

    /// Take the queued sweep for the main loop to run.
    pub fn dispatch(&mut self) -> Option<SweepDispatch> {
        let (request, result) = self.task.dispatch()?;
        let (progress, rx) = watch::channel(SweepPhase::Discovering);
        self.progress = Some((rx, Instant::now()));
        Some(SweepDispatch {
            request,
            result,
            progress,
        })
    }

    /// The finished sweep's outcome, once.
    pub fn try_recv(&mut self) -> Option<anyhow::Result<SweepResult>> {
        let outcome = self.task.try_recv()?;
        self.progress = None;
        Some(outcome)
    }

    pub fn set_result(&mut self, result: SweepResult) {
        self.task.set_result(result);
    }

    pub fn set_error(&mut self) {
        self.task.set_error();
        self.progress = None;
    }

    pub fn result(&self) -> Option<&SweepResult> {
        self.task.result()
    }

    /// The queued (not yet running) sweep.
    pub fn pending(&self) -> Option<&SweepRequest> {
        self.task.pending()
    }

    /// Whether a sweep is queued or running.
    pub fn is_loading(&self) -> bool {
        self.task.is_loading()
    }

    /// How far the running sweep has got. `None` when none is running.
    pub fn progress(&self) -> Option<SweepProgress> {
        let (phase, started) = self.progress.as_ref()?;
        Some(SweepProgress {
            phase: *phase.borrow(),
            elapsed: started.elapsed(),
        })
    }

    pub fn clear(&mut self) {
        self.task.clear();
        self.progress = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> SweepRequest {
        SweepRequest {
            scope: crate::kube::sweep::SweepScope::NotFlux,
            namespace: None,
            config: Default::default(),
        }
    }

    #[test]
    fn progress_follows_the_running_sweep() {
        let mut task = SweepTask::default();
        task.request(request());
        assert_eq!(task.progress(), None, "queued, not running");

        let dispatch = task.dispatch().unwrap();
        assert_eq!(
            task.progress().map(|p| p.phase),
            Some(SweepPhase::Discovering)
        );
        dispatch
            .progress
            .send_replace(SweepPhase::Listing { done: 3, total: 9 });
        assert_eq!(
            task.progress().map(|p| p.phase),
            Some(SweepPhase::Listing { done: 3, total: 9 })
        );

        dispatch.result.send(Ok(SweepResult::default())).unwrap();
        assert!(task.try_recv().is_some());
        assert_eq!(task.progress(), None, "finished");
    }

    #[test]
    fn refresh_keeps_rows_and_request_drops_them() {
        let mut task = SweepTask::default();
        task.set_result(SweepResult {
            kinds_scanned: 4,
            ..SweepResult::default()
        });
        task.refresh(request());
        assert!(task.result().is_some() && task.is_loading());
        task.request(request());
        assert!(task.result().is_none());
    }
}
