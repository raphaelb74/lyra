//! Playback controller.
//!
//! Two threads, no async runtime:
//! - the **controller** owns the decoder, handles commands, and packs audio
//!   into a fixed pool of byte buffers;
//! - the **writer** owns the sink and only moves full buffers to it.
//!
//! The pool is the bounded buffer: the controller waits for an empty buffer
//! before decoding more, so at most [`POOL_SIZE`] blocks are queued. Seek and
//! stop bump a generation counter; the writer skips blocks from older
//! generations, so the device never plays audio from before the seek.

use std::fmt;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle, Thread};
use std::time::Duration;

use format_pcm::Decoder;

use crate::sink::{AudioSink, OutputFormat};
use crate::status::{ErrorKind, NowPlaying, PlaybackState, PlayerError, PlayerStatus};
use crate::writer::{Chunk, ToWriter, Writer, WriterEvent};

/// Decoded blocks in flight. A FLAC block is up to 4096 frames, so this is
/// about 0.75 s at 44.1 kHz and 0.17 s at 192 kHz.
pub const POOL_SIZE: usize = 8;

/// Longest the controller waits for a free buffer before re-checking commands.
const FEED_WAIT: Duration = Duration::from_millis(20);

/// State shared by the controller, the writer and [`Player`] handles.
#[derive(Debug)]
pub(crate) struct Shared {
    pub generation: AtomicU64,
    pub paused: AtomicBool,
    /// Frames handed to the device in the current track.
    pub position: AtomicU64,
    status: Mutex<PlayerStatus>,
}

impl Shared {
    /// Invalidates all queued audio; returns the new generation.
    fn next_generation(&self) -> u64 {
        self.generation.fetch_add(1, Ordering::AcqRel) + 1
    }

    /// State while waiting for audio to reach the device.
    fn waiting_state(&self) -> PlaybackState {
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
    Play { path: PathBuf, device: String },
    Pause,
    Resume,
    Stop,
    Seek { frame: u64 },
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

        let controller = Controller {
            inbox: inbox_rx,
            writer: writer_tx,
            writer_thread: writer_thread.thread().clone(),
            free_rx,
            free_tx,
            shared: Arc::clone(&shared),
            on_change: Box::new(on_change),
            track: None,
            samples: Vec::new(),
        };
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

    /// Plays `path` on the ALSA device `device` (e.g. `hw:CARD=ONE,DEV=0`).
    pub fn play(&self, path: PathBuf, device: String) -> Result<(), PlayerError> {
        self.send(Command::Play { path, device })
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

    /// Moves to `frame` (sample frames from the start of the track).
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
}

impl Player {
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

struct Track {
    decoder: Decoder,
    generation: u64,
    output: Option<OutputFormat>,
    next_frame: u64,
    /// More audio to decode for the writer.
    decoding: bool,
    /// Audio of this generation reached the device.
    started: bool,
}

struct Controller {
    inbox: Receiver<Inbox>,
    writer: Sender<ToWriter>,
    writer_thread: Thread,
    free_rx: Receiver<Vec<u8>>,
    free_tx: Sender<Vec<u8>>,
    shared: Arc<Shared>,
    on_change: Box<dyn Fn(&PlayerStatus) + Send>,
    track: Option<Track>,
    /// Reused decode buffer.
    samples: Vec<i32>,
}

impl Controller {
    fn run(mut self) {
        loop {
            let message = if self.decoding() {
                match self.inbox.try_recv() {
                    Ok(message) => Some(message),
                    Err(TryRecvError::Empty) => None,
                    Err(TryRecvError::Disconnected) => return,
                }
            } else {
                match self.inbox.recv() {
                    Ok(message) => Some(message),
                    Err(_) => return,
                }
            };
            match message {
                Some(Inbox::Command(Command::Shutdown)) => {
                    self.close_track();
                    return;
                }
                Some(Inbox::Command(command)) => self.command(command),
                Some(Inbox::Writer(event)) => self.writer_event(event),
                None => {}
            }
            if self.decoding() {
                self.feed();
            }
        }
    }

    fn decoding(&self) -> bool {
        self.track.as_ref().is_some_and(|track| track.decoding)
    }

    fn command(&mut self, command: Command) {
        match command {
            Command::Play { path, device } => self.load(path, device),
            Command::Pause => {
                if self.track.is_some() {
                    self.shared.paused.store(true, Ordering::Release);
                    self.update(|status| status.state = PlaybackState::Paused);
                }
            }
            Command::Resume => {
                if let Some(track) = &self.track
                    && self.shared.paused.swap(false, Ordering::AcqRel)
                {
                    let state = if track.started {
                        PlaybackState::Playing
                    } else {
                        PlaybackState::Buffering
                    };
                    self.writer_thread.unpark();
                    self.update(|status| status.state = state);
                }
            }
            Command::Stop => {
                self.close_track();
                self.shared.position.store(0, Ordering::Release);
                self.update(|status| *status = PlayerStatus::STOPPED);
            }
            Command::Seek { frame } => self.seek(frame),
            Command::Shutdown => {}
        }
    }

    fn load(&mut self, path: PathBuf, device: String) {
        self.close_track();
        self.shared.paused.store(false, Ordering::Release);
        self.shared.position.store(0, Ordering::Release);
        self.update(|status| {
            *status = PlayerStatus {
                state: PlaybackState::Loading,
                ..PlayerStatus::STOPPED
            }
        });

        let decoder = match Decoder::open(&path) {
            Ok(decoder) => decoder,
            Err(error) => return self.fail(error.into()),
        };
        let source = *decoder.info();
        let generation = self.shared.next_generation();
        self.update(|status| {
            status.state = PlaybackState::Negotiating;
            status.track = Some(NowPlaying {
                path,
                device: device.clone(),
                source,
                output: None,
            });
        });
        self.to_writer(ToWriter::Open {
            generation,
            device,
            stream: source,
        });
        self.track = Some(Track {
            decoder,
            generation,
            output: None,
            next_frame: 0,
            decoding: false,
            started: false,
        });
    }

    fn seek(&mut self, frame: u64) {
        let Some(track) = &mut self.track else { return };
        if track.output.is_none() {
            return;
        }
        if let Err(error) = track.decoder.seek(frame) {
            return self.fail(error.into());
        }
        track.generation = self.shared.next_generation();
        track.next_frame = frame;
        track.decoding = true;
        track.started = false;
        let state = self.shared.waiting_state();
        self.shared.position.store(frame, Ordering::Release);
        self.update(|status| status.state = state);
    }

    fn writer_event(&mut self, event: WriterEvent) {
        let generation = match &event {
            WriterEvent::Opened { generation, .. }
            | WriterEvent::Started { generation }
            | WriterEvent::Drained { generation }
            | WriterEvent::Failed { generation, .. } => *generation,
        };
        let paused = self.shared.paused.load(Ordering::Acquire);
        let waiting = self.shared.waiting_state();
        let Some(track) = self
            .track
            .as_mut()
            .filter(|track| track.generation == generation)
        else {
            return; // About audio that was already stopped or seeked away.
        };
        match event {
            WriterEvent::Opened {
                result: Ok(output), ..
            } => {
                track.output = Some(output);
                track.decoding = true;
                self.update(|status| {
                    status.state = waiting;
                    if let Some(now) = &mut status.track {
                        now.output = Some(output);
                    }
                });
            }
            WriterEvent::Started { .. } => {
                track.started = true;
                if !paused {
                    self.update(|status| status.state = PlaybackState::Playing);
                }
            }
            WriterEvent::Drained { .. } => {
                self.close_track();
                self.update(|status| status.state = PlaybackState::Stopped);
            }
            WriterEvent::Opened {
                result: Err(error), ..
            }
            | WriterEvent::Failed { error, .. } => {
                self.fail(error.into());
            }
        }
    }

    /// Decodes one block into a free buffer and queues it for the writer.
    fn feed(&mut self) {
        let Some(track) = self.track.as_mut().filter(|track| track.decoding) else {
            return;
        };
        let Some(output) = track.output else { return };
        let mut buffer = match self.free_rx.recv_timeout(FEED_WAIT) {
            Ok(buffer) => buffer,
            // Writer busy or paused: check commands, then try again.
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => return,
        };
        match track.decoder.decode_next(&mut self.samples) {
            Ok(true) => {
                buffer.clear();
                output.encoding.pack(&self.samples, &mut buffer);
                let frames = (self.samples.len() / output.channels) as u64;
                let chunk = Chunk {
                    generation: track.generation,
                    start_frame: track.next_frame,
                    frames,
                    bytes: buffer,
                };
                track.next_frame += frames;
                self.to_writer(ToWriter::Chunk(chunk));
            }
            Ok(false) => {
                track.decoding = false;
                let generation = track.generation;
                let _ = self.free_tx.send(buffer);
                self.to_writer(ToWriter::Drain { generation });
            }
            Err(error) => {
                let _ = self.free_tx.send(buffer);
                self.fail(error.into());
            }
        }
    }

    /// Invalidates queued audio and releases the device.
    fn close_track(&mut self) {
        if self.track.take().is_some() {
            self.shared.next_generation();
            self.to_writer(ToWriter::Close);
            self.writer_thread.unpark();
        }
    }

    fn fail(&mut self, error: PlayerError) {
        self.close_track();
        self.update(|status| {
            status.state = PlaybackState::Error;
            status.error = Some(error);
        });
    }

    fn to_writer(&self, message: ToWriter) {
        if self.writer.send(message).is_err() {
            // Only after the writer thread died.
            self.update(|status| {
                status.state = PlaybackState::Error;
                status.error = Some(PlayerError {
                    kind: ErrorKind::Internal,
                    message: "the audio output thread stopped".into(),
                });
            });
        }
    }

    fn update(&self, change: impl FnOnce(&mut PlayerStatus)) {
        let snapshot = {
            let mut status = self
                .shared
                .status
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            change(&mut status);
            status.clone()
        };
        (self.on_change)(&snapshot);
    }
}
