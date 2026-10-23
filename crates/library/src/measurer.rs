//! Measures loudness in the background, one file at a time, at idle CPU and
//! disk priority: the tracks about to play and their albums, nothing else.
//! Runs only while enabled (volume levelling on).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;

use crate::db::{Library, LibraryError};
use crate::measure::measure;

#[derive(Debug)]
pub enum MeasureEvent {
    /// A track's loudness is known now (or known to be unmeasurable).
    Measured { track_id: i64 },
    /// The database could not be read or written; measuring pauses until
    /// the next [`Measurer::poke`].
    Failed(LibraryError),
}

type Listener = dyn Fn(MeasureEvent) + Send + Sync;

/// Cheap to share (`Clone`). The thread starts on the first
/// [`Measurer::set_enabled`]`(true)` and stays for the program's life,
/// asleep when there is nothing to do.
#[derive(Clone)]
pub struct Measurer {
    inner: Arc<Inner>,
}

struct Inner {
    db: PathBuf,
    state: Mutex<State>,
    changed: Condvar,
    /// Raised to abandon the file being measured (levelling turned off).
    stop: AtomicBool,
    listener: Box<Listener>,
}

#[derive(Default)]
struct State {
    enabled: bool,
    started: bool,
    /// Something may be waiting: a scan ended, the queue changed.
    poked: bool,
    /// Tracks about to play, most urgent first.
    first: Vec<i64>,
}

impl std::fmt::Debug for Measurer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Measurer")
            .field("db", &self.inner.db)
            .finish_non_exhaustive()
    }
}

impl Measurer {
    /// `listener` runs on the measuring thread; keep it short.
    pub fn new(db: PathBuf, listener: impl Fn(MeasureEvent) + Send + Sync + 'static) -> Self {
        Self {
            inner: Arc::new(Inner {
                db,
                state: Mutex::new(State::default()),
                changed: Condvar::new(),
                stop: AtomicBool::new(false),
                listener: Box::new(listener),
            }),
        }
    }

    pub fn set_enabled(&self, enabled: bool) {
        let start = {
            let mut state = self.lock();
            state.enabled = enabled;
            state.poked = true;
            let start = enabled && !state.started;
            state.started |= start;
            start
        };
        self.inner.stop.store(!enabled, Ordering::Relaxed);
        self.inner.changed.notify_one();
        if start {
            let inner = Arc::clone(&self.inner);
            let spawned = thread::Builder::new()
                .name("lyra-loudness".into())
                .spawn(move || inner.run());
            if let Err(error) = spawned {
                self.lock().started = false;
                (self.inner.listener)(MeasureEvent::Failed(LibraryError::Io(error)));
            }
        }
    }

    /// Measures these tracks (and their albums) before anything else.
    pub fn prioritise(&self, track_ids: Vec<i64>) {
        let mut state = self.lock();
        state.first = track_ids;
        state.poked = true;
        drop(state);
        self.inner.changed.notify_one();
    }

    /// Looks for work again, after a scan.
    pub fn poke(&self) {
        self.lock().poked = true;
        self.inner.changed.notify_one();
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.inner.lock()
    }
}

impl Inner {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn run(&self) {
        lower_priority();
        let mut library: Option<Library> = None;
        loop {
            let first = {
                let mut state = self.lock();
                while !(state.enabled && state.poked) {
                    state = self
                        .changed
                        .wait(state)
                        .unwrap_or_else(PoisonError::into_inner);
                }
                state.first.clone()
            };
            let step = match &library {
                Some(open) => self.step(open, &first),
                None => {
                    Library::open(&self.db).and_then(|open| self.step(library.insert(open), &first))
                }
            };
            match step {
                Ok(true) => {}
                Ok(false) => self.lock().poked = false,
                Err(error) => {
                    library = None;
                    self.lock().poked = false;
                    (self.listener)(MeasureEvent::Failed(error));
                }
            }
        }
    }

    /// Measures the most urgent track; `false` when none is left.
    fn step(&self, library: &Library, first: &[i64]) -> Result<bool, LibraryError> {
        let Some(track) = library.next_unmeasured(first)? else {
            return Ok(false);
        };
        let blocks = match measure(&track.path, &self.stop) {
            Ok(Some(blocks)) => Some(blocks),
            // Turned off meanwhile: nothing stored, measured again later.
            Ok(None) => return Ok(true),
            // Not measured again until its audio changes. A missing file
            // leaves the library at the next scan.
            Err(_) => None,
        };
        library.store_measurement(track.track_id, &track.audio_id, blocks.as_ref())?;
        (self.listener)(MeasureEvent::Measured {
            track_id: track.track_id,
        });
        Ok(true)
    }
}

/// Idle CPU and disk priority for the calling thread: it runs only when
/// nothing else wants the CPU or the disk.
#[cfg(target_os = "linux")]
fn lower_priority() {
    // ioprio_set(IOPRIO_WHO_PROCESS, this thread, IOPRIO_CLASS_IDLE): the
    // class sits above bit 13.
    const IOPRIO_WHO_PROCESS: libc::c_long = 1;
    const IOPRIO_CLASS_IDLE: libc::c_long = 3 << 13;
    let param = libc::sched_param { sched_priority: 0 };
    // SAFETY: pid 0 is the calling thread; `param` is a valid, initialised
    // struct that outlives the call.
    unsafe {
        libc::sched_setscheduler(0, libc::SCHED_IDLE, &raw const param);
    }
    // SAFETY: a plain syscall with integer arguments; it touches no memory.
    unsafe {
        libc::syscall(
            libc::SYS_ioprio_set,
            IOPRIO_WHO_PROCESS,
            0,
            IOPRIO_CLASS_IDLE,
        );
    }
}

#[cfg(not(target_os = "linux"))]
fn lower_priority() {}
