//! Background scans: one thread at a time, requests coalesced, progress
//! throttled. The thread opens its own database connection.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use crate::db::{Library, LibraryError};
use crate::scan::{ScanProgress, ScanSummary, scan};

/// Longest gap between progress events while files are checked.
const PROGRESS_EVERY: Duration = Duration::from_millis(100);

#[derive(Debug)]
pub enum ScanEvent {
    Progress(ScanProgress),
    /// The scan ended; the library changed if anything was added, updated
    /// or removed.
    Finished(Result<ScanSummary, LibraryError>),
}

type Listener = dyn Fn(ScanEvent) + Send + Sync;

/// Starts scans on request. Cheap to share (`Clone`).
#[derive(Clone)]
pub struct Scanner {
    inner: Arc<Inner>,
}

struct Inner {
    db: PathBuf,
    art: PathBuf,
    /// `(running, again)`: a request while running schedules one more scan.
    state: Mutex<(bool, bool)>,
    cancel: AtomicBool,
    listener: Box<Listener>,
}

impl std::fmt::Debug for Scanner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Scanner")
            .field("db", &self.inner.db)
            .field("running", &self.is_running())
            .finish_non_exhaustive()
    }
}

impl Scanner {
    /// `listener` runs on the scan thread; keep it short.
    pub fn new(
        db: PathBuf,
        art: PathBuf,
        listener: impl Fn(ScanEvent) + Send + Sync + 'static,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                db,
                art,
                state: Mutex::new((false, false)),
                cancel: AtomicBool::new(false),
                listener: Box::new(listener),
            }),
        }
    }

    /// Scans all folders now, or once more after the running scan.
    pub fn request(&self) {
        {
            let mut state = self.lock();
            if state.0 {
                state.1 = true;
                return;
            }
            *state = (true, false);
        }
        self.inner.cancel.store(false, Ordering::Relaxed);
        let inner = Arc::clone(&self.inner);
        let spawned = thread::Builder::new()
            .name("lyra-scan".into())
            .spawn(move || inner.run());
        if let Err(error) = spawned {
            *self.lock() = (false, false);
            (self.inner.listener)(ScanEvent::Finished(Err(LibraryError::Io(error))));
        }
    }

    /// Stops the running scan after the current file; nothing queued runs.
    pub fn cancel(&self) {
        self.lock().1 = false;
        self.inner.cancel.store(true, Ordering::Relaxed);
    }

    pub fn is_running(&self) -> bool {
        self.lock().0
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, (bool, bool)> {
        self.inner
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

impl Inner {
    fn run(&self) {
        loop {
            let result = self.scan_once();
            (self.listener)(ScanEvent::Finished(result));
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            if state.1 && !self.cancel.load(Ordering::Relaxed) {
                state.1 = false;
            } else {
                *state = (false, false);
                return;
            }
        }
    }

    fn scan_once(&self) -> Result<ScanSummary, LibraryError> {
        let mut library = Library::open(&self.db)?;
        let mut last = None::<Instant>;
        scan(&mut library, &self.art, &self.cancel, |progress| {
            let due = last.is_none_or(|at| at.elapsed() >= PROGRESS_EVERY);
            if due || progress.done == progress.total {
                last = Some(Instant::now());
                (self.listener)(ScanEvent::Progress(progress));
            }
        })
    }
}
