//! The controller thread: queue, source, commands, and the gapless handoff.

use std::collections::VecDeque;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::{Arc, PoisonError};
use std::thread::Thread;
use std::time::Duration;

use crate::player::{Command, Inbox, QueueItem, Shared};
use crate::sink::{DsdTransport, Encoding, HardwareVolume, OutputFormat};
use crate::source::{Source, SourceInfo};
use crate::status::{ErrorKind, NowPlaying, PlaybackState, PlayerError, PlayerStatus, format_db};
use crate::writer::{Chunk, ToWriter, WriterEvent};

/// Longest the controller waits for a free buffer before re-checking commands.
const FEED_WAIT: Duration = Duration::from_millis(20);

/// "Previous" restarts the current track once it has played this long.
const RESTART_AFTER_SECONDS: u64 = 3;

/// A queue being played on one device.
struct Session {
    items: Arc<[QueueItem]>,
    device: String,
    dsd: DsdTransport,
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
    seq: u64,
    index: usize,
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
            Command::Shutdown => {}
        }
    }

    /// The queue, shown entry, device and DSD policy of the session.
    fn current(&self) -> Option<(Arc<[QueueItem]>, usize, String, DsdTransport)> {
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
        dsd: DsdTransport,
    ) {
        let Some(item) = items.get(index).cloned() else {
            return self.stop();
        };
        let mut source = match Source::open(&item.path) {
            Ok(source) => source,
            Err(error) => return self.fail(error),
        };
        if frame > 0
            && let Err(error) = source.seek(frame)
        {
            return self.fail(error);
        }
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
                seq: entry.seq,
                index,
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
                if let Err(error) = dop_allowed(output.encoding, output.hardware_volume) {
                    return self.fail(error);
                }
                session.output = Some(output);
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
        match feed.source.next(output.encoding, &mut buffer) {
            Ok(Some(frames)) => {
                let chunk = Chunk {
                    generation: session.generation,
                    seq: feed.seq,
                    start_frame: feed.next_frame,
                    frames,
                    bytes: buffer,
                };
                feed.next_frame += frames;
                self.to_writer(ToWriter::Chunk(chunk));
            }
            Ok(None) => {
                let _ = self.free_tx.send(buffer);
                self.track_ended(output);
            }
            Err(error) => {
                let _ = self.free_tx.send(buffer);
                self.fail(error);
            }
        }
    }

    /// The decoded track ended: continue gaplessly into the next entry when
    /// the open device carries it, otherwise let the device drain.
    fn track_ended(&mut self, output: OutputFormat) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        let next = session.feed.index + 1;
        if let Some(item) = session.items.get(next) {
            // A file that fails to open is reported when its turn comes.
            if let Ok(mut source) = Source::open(&item.path) {
                let info = source.info();
                if output.carries(&info.stream(session.dsd)) {
                    source.continue_from(&session.feed.source);
                    self.seq += 1;
                    session.upcoming.push_back(Entry {
                        seq: self.seq,
                        index: next,
                        info,
                    });
                    session.feed = Feed {
                        seq: self.seq,
                        index: next,
                        source,
                        next_frame: 0,
                        decoding: true,
                    };
                    return;
                }
            }
            session.after_drain = Some(next);
        }
        session.feed.decoding = false;
        let generation = session.generation;
        self.to_writer(ToWriter::Drain { generation });
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

/// DoP only reaches a DAC whose volume is known to be untouched. DoP is DSD
/// dressed as PCM: a device that does not decode it (built-in audio, most
/// non-USB outputs) plays it as loud noise, and attenuation breaks the
/// markers. A USB DAC at 0 dB (or without volume control) is the safe case.
///
/// Native DSD has no such risk: the device was opened in a DSD format.
fn dop_allowed(encoding: Encoding, volume: HardwareVolume) -> Result<(), PlayerError> {
    let message = match (encoding, volume) {
        (Encoding::Dop(_), HardwareVolume::Unknown) => {
            "DSD over DoP is only sent to a USB DAC whose volume Lyra can check. \
             This device is not one: if it does not decode DoP (built-in audio cannot), \
             it would play loud noise. Lyra does not convert DSD to PCM."
                .to_owned()
        }
        (Encoding::Dop(_), HardwareVolume::Attenuated { centibels }) => format!(
            "DSD over DoP needs the DAC's volume at 0 dB; it is at {} dB. \
             Raise it to maximum (for example in alsamixer) and play again.",
            format_db(centibels)
        ),
        _ => return Ok(()),
    };
    Err(PlayerError {
        kind: ErrorKind::Unsupported,
        message,
    })
}
