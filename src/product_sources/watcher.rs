use super::TaskSourceAdapter;
use crate::product_contract::{AdapterError, CapturedProgram};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

const SETTLE: Duration = Duration::from_millis(120);
const RECHECK: Duration = Duration::from_secs(1);
#[derive(Debug)]
pub enum SourceUpdate {
    Unchanged,
    Pending(String),
    Captured(Box<CapturedProgram>),
    Unavailable(AdapterError),
}
/// Events are coalesced, not queued without bounds. A periodic reconciliation
/// covers missed events; every actual result still comes from a fresh capture.
pub struct SourceWatcher {
    adapter: TaskSourceAdapter,
    _watcher: RecommendedWatcher,
    dirty: Arc<AtomicBool>,
    error: Arc<Mutex<Option<String>>>,
    pending: Option<(CapturedProgram, Instant)>,
    retry: bool,
    last_check: Instant,
    last: Option<CapturedProgram>,
}
impl SourceWatcher {
    pub fn new(adapter: TaskSourceAdapter) -> Result<Self, AdapterError> {
        let dirty = Arc::new(AtomicBool::new(true));
        let error = Arc::new(Mutex::new(None));
        let event_dirty = dirty.clone();
        let event_error = error.clone();
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| match event {
                Ok(event) if !matches!(event.kind, EventKind::Access(_)) => {
                    event_dirty.store(true, Ordering::Release);
                }
                Err(e) => {
                    if let Ok(mut slot) = event_error.lock() {
                        *slot = Some(e.to_string());
                    }
                    event_dirty.store(true, Ordering::Release);
                }
                _ => (),
            })
            .map_err(|e| AdapterError::Failed(format!("Source watcher unavailable: {e}")))?;
        watcher
            .watch(adapter.worktree(), RecursiveMode::Recursive)
            .map_err(|e| AdapterError::Failed(e.to_string()))?;
        Ok(Self {
            adapter,
            _watcher: watcher,
            dirty,
            error,
            pending: None,
            retry: true,
            last_check: Instant::now(),
            last: None,
        })
    }
    pub fn last_capture(&self) -> Option<&CapturedProgram> {
        self.last.as_ref()
    }
    /// Call on a worker, not the UI thread: fingerprinting walks the complete
    /// task source. Call ensure_fresh again after analysis and before adoption.
    pub fn poll(&mut self) -> SourceUpdate {
        if let Ok(mut slot) = self.error.lock() {
            if let Some(error) = slot.take() {
                return SourceUpdate::Unavailable(AdapterError::Failed(format!(
                    "Source watcher lost events: {error}"
                )));
            }
        }
        if self.dirty.swap(false, Ordering::AcqRel) {
            self.retry = true;
        }
        if !self.retry && self.last_check.elapsed() < RECHECK {
            return SourceUpdate::Unchanged;
        }
        self.last_check = Instant::now();
        let captured = match self.adapter.capture_current() {
            Ok(captured) => captured,
            Err(error) => {
                self.pending = None;
                self.retry = true;
                return match error {
                    AdapterError::Invalid(ref e) => SourceUpdate::Pending(format!(
                        "Program is incomplete or invalid; no result accepted: {e}"
                    )),
                    other => SourceUpdate::Unavailable(other),
                };
            }
        };
        if self.last.as_ref().is_some_and(|last| last == &captured) {
            self.pending = None;
            self.retry = false;
            return SourceUpdate::Unchanged;
        }
        if self
            .pending
            .as_ref()
            .is_some_and(|(prior, since)| prior == &captured && since.elapsed() >= SETTLE)
        {
            self.pending = None;
            self.retry = false;
            self.last = Some(captured.clone());
            return SourceUpdate::Captured(Box::new(captured));
        }
        if self
            .pending
            .as_ref()
            .is_none_or(|(prior, _)| prior != &captured)
        {
            self.pending = Some((captured, Instant::now()));
        }
        self.retry = true;
        SourceUpdate::Pending("Waiting for a stable complete program".into())
    }
}
