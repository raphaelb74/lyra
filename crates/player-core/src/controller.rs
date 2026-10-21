//! The controller thread: queue, source, commands, and the gapless handoff.

use std::collections::VecDeque;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::{Arc, PoisonError};
use std::thread::Thread;
use std::time::Duration;

use crate::dop;
use crate::player::{Command, Inbox, QueueItem, Shared};
use crate::sink::{DsdPolicy, Encoding, HardwareVolume, OutputFormat};
use crate::source::{Source, SourceInfo};
use crate::status::{ErrorKind, NowPlaying, PlaybackState, PlayerError, PlayerStatus};
use crate::writer::{Chunk, ToWriter, WriterEvent};

/// Longest the controller waits for a free buffer before re-checking commands.
const FEED_WAIT: Duration = Duration::from_millis(20);

/// "Previous" restarts the current track once it has played this long.
const RESTART_AFTER_SECONDS: u64 = 3;

/// A queue being played on one device.
struct Session {
    items: Arc<[QueueItem]>,
    device: String,
    dsd: DsdPolicy,
    generation: u64,
    /// Format the device is open with; `None` while it opens.
    output: Option<OutputFormat>,
    /// Audio of this generation reached the device.
    started: bool,
    /// The track being decoded; ahead of the one playing near a boundary.
    feed: Feed,
    /// Tracks already handed to the writer whose audio has not started yet.
    upcoming: VecDeque<Entry>,
    /// The track the device is playing (what the status shows).
    shown: Entry,
    /// Entry to open once the device drains: the next track needs another
    /// format, so it cannot follow gaplessly.
    after_drain: Option<usize>,
}

struct Feed {
    /// The track being decoded.
    entry: Entry,
    source: Source,
    next_frame: u64,
    /// More audio to decode for the writer.
    decoding: bool,
}

/// One started track: `seq` is unique per start, so the writer can say
/// which track its audio belongs to.
#[derive(Clone, Copy)]
struct Entry {
    seq: u64,
    index: usize,
    info: SourceInfo,
}

pub(crate) struct Controller {
    inbox: Receiver<Inbox>,
    writer: Sender<ToWriter>,
    writer_thread: Thread,
    free_rx: Receiver<Vec<u8>>,
    free_tx: Sender<Vec<u8>>,
    shared: Arc<Shared>,
    on_change: Box<dyn Fn(&PlayerStatus) + Send>,
    session: Option<Session>,
    /// Last track sequence number handed out.
    seq: u64,
    /// Device and format last opened: the pre-roll is only needed when it
    /// changes (or on the first open, when the DAC's state is unknown).
    last_format: Option<(String, Encoding, u32)>,
}

impl Controller {
    pub fn new(
        inbox: Receiver<Inbox>,
        writer: Sender<ToWriter>,
        writer_thread: Thread,
        free_rx: Receiver<Vec<u8>>,
        free_tx: Sender<Vec<u8>>,
        shared: Arc<Shared>,
        on_change: Box<dyn Fn(&PlayerStatus) + Send>,
    ) -> Self {
        Self {
            inbox,
            writer,
            writer_thread,
            free_rx,
            free_tx,
            shared,
            on_change,
            session: None,
            seq: 0,
            last_format: None,
        }
    }

    pub fn run(mut self) {
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
                    self.close_session();
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
        self.session
            .as_ref()
            .is_some_and(|session| session.feed.decoding && session.output.is_some())
    }

    fn command(&mut self, command: Command) {
        match command {
            Command::PlayQueue {
                items,
                start,
                device,
                dsd,
            } => {
                self.update(|status| {
                    *status = PlayerStatus {
                        state: PlaybackState::Loading,
                        queue_length: items.len(),
                        ..PlayerStatus::STOPPED
                    }
                });
                self.begin(Arc::from(items), start, 0, device, dsd);
            }
            Command::Next => {
                if let Some((items, index, device, dsd)) = self.current() {
                    if index + 1 < items.len() {
                        self.begin(items, index + 1, 0, device, dsd);
                    } else {
                        self.stop();
                    }
                }
            }
            Command::Previous => {
                if let Some((items, index, device, dsd)) = self.current() {
                    let restart = index == 0 || self.played_seconds() >= RESTART_AFTER_SECONDS;
                    let target = if restart { index } else { index - 1 };
                    self.begin(items, target, 0, device, dsd);
                }
            }
            Command::SkipTo { index } => {
                if let Some((items, _, device, dsd)) = self.current()
                    && index < items.len()
                {
                    self.begin(items, index, 0, device, dsd);
                }
            }
            Command::Seek { frame } => {
                // Restarting the shown track at `frame` also covers a seek
                // just after a gapless boundary, when the decoder is already
                // in the next track.
                if let Some((items, index, device, dsd)) = self.current()
                    && self.session.as_ref().is_some_and(|s| s.output.is_some())
                {
                    self.begin(items, index, frame, device, dsd);
                }
            }
            Command::Pause => {
                if self.session.is_some() {
                    self.shared.paused.store(true, Ordering::Release);
                    self.update(|status| status.state = PlaybackState::Paused);
                }
            }
            Command::Resume => {
                if let Some(session) = &self.session
                    && self.shared.paused.swap(false, Ordering::AcqRel)
                {
                    let state = if session.started {
                        PlaybackState::Playing
                    } else {
                        PlaybackState::Buffering
                    };
                    self.writer_thread.unpark();
                    self.update(|status| status.state = state);
                }
            }
            Command::Stop => self.stop(),
            Command::VolumeChanged { device, volume } => self.volume_changed(&device, volume),
            Command::Shutdown => {}
        }
    }

    /// The queue, shown entry, device and DSD policy of the session.
    fn current(&self) -> Option<(Arc<[QueueItem]>, usize, String, DsdPolicy)> {
        self.session.as_ref().map(|session| {
            (
                Arc::clone(&session.items),
                session.shown.index,
                session.device.clone(),
                session.dsd,
            )
        })
    }

    /// Whole seconds of the shown track handed to the device.
    fn played_seconds(&self) -> u64 {
        let rate = self
            .session
            .as_ref()
            .map_or(0, |session| session.shown.info.frame_rate());
        self.shared.position.load(Ordering::Acquire) / u64::from(rate.max(1))
    }

    /// Starts queue entry `index` at `frame`. Keeps the device open when its
    /// format carries the track (fast skip and seek); otherwise reopens it.
    fn begin(
        &mut self,
        items: Arc<[QueueItem]>,
        index: usize,
        frame: u64,
        device: String,
        dsd: DsdPolicy,
    ) {
        let Some(item) = items.get(index).cloned() else {
            return self.stop();
        };
        let source = match open_at(&item, frame) {
            Ok(source) => source,
            Err(error) => return self.fail(error),
        };
        let info = source.info();
        let stream = info.stream(dsd);
        let reuse = self
            .session
            .as_ref()
            .filter(|session| session.device == device)
            .and_then(|session| session.output)
            .filter(|output| output.carries(&stream));

        let generation = self.shared.next_generation();
        self.seq += 1;
        let entry = Entry {
            seq: self.seq,
            index,
            info,
        };
        self.shared.paused.store(false, Ordering::Release);
        self.shared.position.store(frame, Ordering::Release);
        *self
            .shared
            .queue
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Arc::clone(&items);
        if reuse.is_none() {
            self.to_writer(ToWriter::Open {
                generation,
                device: device.clone(),
                stream,
            });
        }
        // A paused writer wakes for the new generation.
        self.writer_thread.unpark();

        let state = if reuse.is_some() {
            self.shared.waiting_state()
        } else {
            PlaybackState::Negotiating
        };
        let now = now_playing(&item, &entry, &device, reuse);
        let length = items.len();
        self.update(|status| {
            *status = PlayerStatus {
                state,
                track: Some(now),
                error: None,
                queue_length: length,
            };
        });
        self.session = Some(Session {
            items,
            device,
            dsd,
            generation,
            output: reuse,
            started: false,
            feed: Feed {
                entry,
                source,
                next_frame: frame,
                decoding: true,
            },
            upcoming: VecDeque::new(),
            shown: entry,
            after_drain: None,
        });
    }

    fn writer_event(&mut self, event: WriterEvent) {
        let generation = match &event {
            WriterEvent::Opened { generation, .. }
            | WriterEvent::Started { generation, .. }
            | WriterEvent::Drained { generation }
            | WriterEvent::Failed { generation, .. } => *generation,
        };
        let paused = self.shared.paused.load(Ordering::Acquire);
        let waiting = self.shared.waiting_state();
        let Some(session) = self
            .session
            .as_mut()
            .filter(|session| session.generation == generation)
        else {
            return; // About audio that was already stopped, skipped or seeked away.
        };
        match event {
            WriterEvent::Opened {
                result: Ok(output), ..
            } => {
                if let Err(error) = dop::check_open(&output) {
                    return self.fail(error);
                }
                session.output = Some(output);
                let format = (session.device.clone(), output.encoding, output.sample_rate);
                let resync = Duration::from_millis(self.shared.resync_ms.load(Ordering::Acquire));
                if !resync.is_zero() && self.last_format.as_ref() != Some(&format) {
                    self.to_writer(ToWriter::Preroll {
                        generation,
                        duration: resync,
                    });
                }
                self.last_format = Some(format);
                self.update(|status| {
                    status.state = waiting;
                    if let Some(now) = &mut status.track {
                        now.output = Some(output);
                    }
                });
            }
            WriterEvent::Started { seq, .. } => {
                session.started = true;
                let mut reached = None;
                while let Some(next) = session.upcoming.front().copied() {
                    if next.seq > seq {
                        break;
                    }
                    session.upcoming.pop_front();
                    reached = Some(next);
                }
                if let Some(entry) = reached {
                    session.shown = entry;
                    let item = session.items[entry.index].clone();
                    let now = now_playing(&item, &entry, &session.device, session.output);
                    self.update(|status| status.track = Some(now));
                }
                if !paused {
                    self.update(|status| status.state = PlaybackState::Playing);
                }
            }
            WriterEvent::Drained { .. } => {
                if let Some(index) = session.after_drain {
                    // The next track needs another format: reopen for it.
                    let (items, device, dsd) = (
                        Arc::clone(&session.items),
                        session.device.clone(),
                        session.dsd,
                    );
                    self.begin(items, index, 0, device, dsd);
                } else {
                    self.close_session();
                    self.update(|status| status.state = PlaybackState::Stopped);
                }
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
        let Some(session) = self.session.as_mut() else {
            return;
        };
        let Some(output) = session.output else { return };
        let mut buffer = match self.free_rx.recv_timeout(FEED_WAIT) {
            Ok(buffer) => buffer,
            // Writer busy or paused: check commands, then try again.
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => return,
        };
        let feed = &mut session.feed;
        match feed.source.next(&output, session.dsd, &mut buffer) {
            Ok(Some(frames)) => {
                let chunk = Chunk {
                    generation: session.generation,
                    seq: feed.entry.seq,
                    start_frame: feed.next_frame,
                    frames,
                    bytes: buffer,
                };
                feed.next_frame += frames;
                self.to_writer(ToWriter::Chunk(chunk));
            }
            Ok(None) => self.track_ended(output, buffer),
            Err(error) => {
                self.recycle(buffer);
                self.fail(error);
            }
        }
    }

    /// The decoded track ended: continue gaplessly into the next entry when
    /// the open device carries it, otherwise let the device drain. `buffer`
    /// is a free buffer, for the end of converted audio.
    fn track_ended(&mut self, output: OutputFormat, buffer: Vec<u8>) {
        let Some(session) = self.session.as_mut() else {
            return self.recycle(buffer);
        };
        let next = session.feed.entry.index + 1;
        let continuation = session
            .items
            .get(next)
            // A file that fails to open is reported when its turn comes.
            .and_then(|item| Source::open(&item.path).ok())
            .filter(|source| output.carries(&source.info().stream(session.dsd)));

        let Some(mut source) = continuation else {
            if next < session.items.len() {
                session.after_drain = Some(next);
            }
            session.feed.decoding = false;
            let generation = session.generation;
            self.send_tail(output, buffer);
            return self.to_writer(ToWriter::Drain { generation });
        };
        source.continue_from(&mut session.feed.source);
        // The previous track's converter tail, if it did not carry over.
        self.send_tail(output, buffer);
        self.seq += 1;
        let entry = Entry {
            seq: self.seq,
            index: next,
            info: source.info(),
        };
        if let Some(session) = self.session.as_mut() {
            session.upcoming.push_back(entry);
            session.feed = Feed {
                entry,
                source,
                next_frame: 0,
                decoding: true,
            };
        }
    }

    /// Queues what the converter still holds of the track that ended (its
    /// filters' tail); returns the buffer to the pool when there is none.
    fn send_tail(&mut self, output: OutputFormat, mut buffer: Vec<u8>) {
        let Some(session) = self.session.as_mut() else {
            return self.recycle(buffer);
        };
        session.feed.source.tail(output.encoding, &mut buffer);
        if buffer.is_empty() {
            return self.recycle(buffer);
        }
        let chunk = Chunk {
            generation: session.generation,
            seq: session.feed.entry.seq,
            start_frame: session.feed.next_frame,
            frames: 0,
            bytes: buffer,
        };
        self.to_writer(ToWriter::Chunk(chunk));
    }

    fn volume_changed(&mut self, device: &str, volume: HardwareVolume) {
        let Some(session) = self.session.as_mut().filter(|s| s.device == device) else {
            return;
        };
        let Some(mut output) = session.output.filter(|o| o.hardware_volume != volume) else {
            return;
        };
        if let Err(error) = dop::check_change(&output, volume) {
            return self.fail(error);
        }
        output.hardware_volume = volume;
        session.output = Some(output);
        self.update(|status| {
            if let Some(shown) = status.track.as_mut().and_then(|now| now.output.as_mut()) {
                shown.hardware_volume = volume;
            }
        });
    }

    /// Returns a buffer to the pool. It only fails at shutdown, when there is
    /// nothing to recycle into.
    fn recycle(&self, buffer: Vec<u8>) {
        let _ = self.free_tx.send(buffer);
    }

    fn stop(&mut self) {
        self.close_session();
        self.shared.position.store(0, Ordering::Release);
        let length = self
            .shared
            .queue
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len();
        self.update(|status| {
            *status = PlayerStatus {
                queue_length: length,
                ..PlayerStatus::STOPPED
            }
        });
    }

    /// Invalidates queued audio and releases the device.
    fn close_session(&mut self) {
        if self.session.take().is_some() {
            self.shared.next_generation();
            self.to_writer(ToWriter::Close);
            self.writer_thread.unpark();
        }
    }

    fn fail(&mut self, error: PlayerError) {
        self.close_session();
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

fn now_playing(
    item: &QueueItem,
    entry: &Entry,
    device: &str,
    output: Option<OutputFormat>,
) -> NowPlaying {
    NowPlaying {
        path: item.path.clone(),
        id: item.id,
        index: entry.index,
        device: device.to_owned(),
        source: entry.info,
        output,
    }
}

/// Opens `item`, at `frame` source frames from its start.
fn open_at(item: &QueueItem, frame: u64) -> Result<Source, PlayerError> {
    let mut source = Source::open(&item.path)?;
    if frame > 0 {
        source.seek(frame)?;
    }
    Ok(source)
}
