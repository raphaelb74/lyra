//! Playback engine handle.
//!
//! Two threads, no async runtime:
//! - the **controller** (`controller.rs`) owns the queue and the source
//!   (decoder or DSD reader), handles commands, and packs audio into a fixed
//!   pool of byte buffers;
//! - the **writer** owns the sink and only moves full buffers to it.
//!
//! The pool is the bounded buffer: the controller waits for an empty buffer
//! before decoding more, so at most [`POOL_SIZE`] blocks are queued. Seek,
//! skip and stop bump a generation counter; the writer skips blocks from
//! older generations, so the device never plays audio from before the change.
//!
//! Consecutive tracks the open device can carry unchanged play gaplessly:
//! the controller opens the next file when the current one ends and keeps
//! feeding the same stream.

use std::fmt;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};

use crate::controller::Controller;
use crate::sink::{AudioSink, DsdPolicy};
use crate::status::{ErrorKind, PlaybackState, PlayerError, PlayerStatus};
use crate::writer::{Writer, WriterEvent};

/// Decoded blocks in flight. A FLAC block is up to 4096 frames, so this is
/// about 0.75 s at 44.1 kHz and 0.17 s at 192 kHz.
pub const POOL_SIZE: usize = 8;

/// One entry of the play queue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueueItem {
    pub path: PathBuf,
    /// The caller's id (e.g. a library track id), echoed in
    /// [`crate::NowPlaying::id`].
    pub id: Option<i64>,
}

/// State shared by the controller, the writer and [`Player`] handles.
#[derive(Debug)]
pub(crate) struct Shared {
    pub generation: AtomicU64,
    pub paused: AtomicBool,
    /// Frames handed to the device in the current track.
    pub position: AtomicU64,
    pub status: Mutex<PlayerStatus>,
    /// The queue, for [`Player::queue`]; replaced as a whole.
    pub queue: Mutex<Arc<[QueueItem]>>,
}

impl Shared {
    /// Invalidates all queued audio; returns the new generation.
    pub fn next_generation(&self) -> u64 {
        self.generation.fetch_add(1, Ordering::AcqRel) + 1
    }

    /// State while waiting for audio to reach the device.
    pub fn waiting_state(&self) -> PlaybackState {
        if self.paused.load(Ordering::Acquire) {
            PlaybackState::Paused
        } else {
            PlaybackState::Buffering
        }
    }
}

#[derive(Debug)]
pub(crate) enum Inbox {
    Command(Command),
    Writer(WriterEvent),
}

#[derive(Debug)]
pub(crate) enum Command {
    PlayQueue {
        items: Vec<QueueItem>,
        start: usize,
        device: String,
        dsd: DsdPolicy,
    },
    Next,
    Previous,
    SkipTo {
        index: usize,
    },
    Pause,
    Resume,
    Stop,
    Seek {
        frame: u64,
    },
    Shutdown,
}

/// Handle to the playback engine. Dropping it stops playback and joins the
/// engine threads.
pub struct Player {
    inbox: Sender<Inbox>,
    shared: Arc<Shared>,
    controller: Mutex<Option<JoinHandle<()>>>,
}

impl fmt::Debug for Player {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Player")
            .field("shared", &self.shared)
            .finish_non_exhaustive()
    }
}

impl Player {
    /// Starts the engine. `on_change` runs on the controller thread after
    /// every state change; keep it short (forward the snapshot and return).
    pub fn new<S: AudioSink>(
        sink: S,
        on_change: impl Fn(&PlayerStatus) + Send + 'static,
    ) -> io::Result<Self> {
        let shared = Arc::new(Shared {
            generation: AtomicU64::new(0),
            paused: AtomicBool::new(false),
            position: AtomicU64::new(0),
            status: Mutex::new(PlayerStatus::STOPPED),
            queue: Mutex::new(Arc::from([])),
        });
        let (inbox_tx, inbox_rx) = mpsc::channel();
        let (writer_tx, writer_rx) = mpsc::channel();
        let (free_tx, free_rx) = mpsc::channel();
        for _ in 0..POOL_SIZE {
            // The receiver is alive: it is held right here.
            let _ = free_tx.send(Vec::new());
        }

        let writer = Writer {
            sink,
            input: writer_rx,
            free: free_tx.clone(),
            events: inbox_tx.clone(),
            shared: Arc::clone(&shared),
        };
        let writer_thread = thread::Builder::new()
            .name("lyra-writer".into())
            .spawn(move || writer.run())?;

        let controller = Controller::new(
            inbox_rx,
            writer_tx,
            writer_thread.thread().clone(),
            free_rx,
            free_tx,
            Arc::clone(&shared),
            Box::new(on_change),
        );
        let controller = thread::Builder::new()
            .name("lyra-player".into())
            .spawn(move || {
                controller.run();
                // The writer exits once the controller drops its sender.
                let _ = writer_thread.join();
            })?;
        Ok(Self {
            inbox: inbox_tx,
            shared,
            controller: Mutex::new(Some(controller)),
        })
    }

    /// Plays one file on the ALSA device `device` (e.g. `hw:CARD=ONE,DEV=0`):
    /// a queue of one. `dsd` chooses how DSD files reach the device.
    pub fn play(&self, path: PathBuf, device: String, dsd: DsdPolicy) -> Result<(), PlayerError> {
        self.play_queue(vec![QueueItem { path, id: None }], 0, device, dsd)
    }

    /// Replaces the queue and plays it from `start`; continues through the
    /// following entries, gaplessly where the format allows.
    pub fn play_queue(
        &self,
        items: Vec<QueueItem>,
        start: usize,
        device: String,
        dsd: DsdPolicy,
    ) -> Result<(), PlayerError> {
        if start >= items.len() {
            return Err(PlayerError {
                kind: ErrorKind::Internal,
                message: "nothing to play at that queue position".into(),
            });
        }
        self.send(Command::PlayQueue {
            items,
            start,
            device,
            dsd,
        })
    }

    /// Plays the next queue entry; stops after the last.
    pub fn next(&self) -> Result<(), PlayerError> {
        self.send(Command::Next)
    }

    /// Restarts the track, or plays the previous entry when the track has
    /// barely begun.
    pub fn previous(&self) -> Result<(), PlayerError> {
        self.send(Command::Previous)
    }

    /// Plays queue entry `index`.
    pub fn skip_to(&self, index: usize) -> Result<(), PlayerError> {
        self.send(Command::SkipTo { index })
    }

    pub fn pause(&self) -> Result<(), PlayerError> {
        self.send(Command::Pause)
    }

    pub fn resume(&self) -> Result<(), PlayerError> {
        self.send(Command::Resume)
    }

    pub fn stop(&self) -> Result<(), PlayerError> {
        self.send(Command::Stop)
    }

    /// Moves to `frame` (source frames from the start of the current track).
    pub fn seek(&self, frame: u64) -> Result<(), PlayerError> {
        self.send(Command::Seek { frame })
    }

    pub fn status(&self) -> PlayerStatus {
        self.shared
            .status
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The current queue; [`crate::NowPlaying::index`] points into it.
    pub fn queue(&self) -> Arc<[QueueItem]> {
        Arc::clone(
            &self
                .shared
                .queue
                .lock()
                .unwrap_or_else(PoisonError::into_inner),
        )
    }

    /// Frames of the current track handed to the device. Ahead of what is
    /// audible by the device buffer (tens of milliseconds).
    pub fn position(&self) -> u64 {
        self.shared.position.load(Ordering::Acquire)
    }

    fn send(&self, command: Command) -> Result<(), PlayerError> {
        self.inbox
            .send(Inbox::Command(command))
            .map_err(|_| PlayerError {
                kind: ErrorKind::Internal,
                message: "the playback engine has stopped".into(),
            })
    }

    /// Stops playback, releases the device (restoring anything the sink
    /// changed) and joins the engine threads. Later commands fail. Called by
    /// `Drop`; call it explicitly where `Drop` may not run (app exit).
    pub fn shutdown(&self) {
        let _ = self.inbox.send(Inbox::Command(Command::Shutdown));
        let controller = self
            .controller
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(controller) = controller {
            let _ = controller.join();
        }
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.shutdown();
    }
}
