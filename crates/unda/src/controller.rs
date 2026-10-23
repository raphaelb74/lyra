//! The controller thread: queue, source, commands, and the gapless handoff.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::sync::{Arc, PoisonError};
use std::thread::{self, Thread};
use std::time::Duration;

use crate::dop;
use crate::order::{Edit, Moved, Queue, Repeat, following};
use crate::player::{Command, Inbox, POOL_SIZE, QueueItem, Shared, UNKNOWN};
use crate::sink::{DsdPolicy, Encoding, HardwareVolume, OutputFormat};
use crate::source::{Source, SourceInfo};
use crate::status::{ErrorKind, NowPlaying, PlaybackState, PlayerError, PlayerStatus};
use crate::writer::{Chunk, ToWriter, WriterEvent};

/// "Previous" restarts the current track once it has played this long.
const RESTART_AFTER_SECONDS: u64 = 3;

/// A queue being played on one device.
struct Session {
    queue: Queue,
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

impl Session {
    /// Positions already playing or decoded, in play order: queue changes
    /// keep them where they are.
    fn committed(&self) -> Vec<usize> {
        let mut committed = vec![self.shown.index];
        committed.extend(self.upcoming.iter().map(|entry| entry.index));
        committed.push(self.feed.entry.index);
        committed.dedup();
        committed
    }
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

/// The next track, opened before its turn so a slow disk or network share
/// does not hold up the gapless handoff.
enum Prepared {
    Opening {
        path: PathBuf,
        token: u64,
    },
    Ready {
        path: PathBuf,
        source: Box<Source>,
    },
    /// Not tried again: the track opens (and reports why not) on its turn.
    Failed {
        path: PathBuf,
    },
}

impl Prepared {
    fn path(&self) -> &Path {
        match self {
            Self::Opening { path, .. } | Self::Ready { path, .. } | Self::Failed { path } => path,
        }
    }
}

pub(crate) struct Controller {
    inbox: Receiver<Inbox>,
    /// For the threads that open tracks ahead.
    inbox_tx: Sender<Inbox>,
    writer: Sender<ToWriter>,
    writer_thread: Thread,
    /// Empty buffers: the pool. The writer holds the others.
    free: Vec<Vec<u8>>,
    shared: Arc<Shared>,
    on_change: Box<dyn Fn(&PlayerStatus) + Send>,
    session: Option<Session>,
    /// Last track sequence number handed out.
    seq: u64,
    /// Device and format last opened: the pre-roll is only needed when it
    /// changes (or on the first open, when the DAC's state is unknown).
    last_format: Option<(String, Encoding, u32)>,
    /// Kept across queues, like the listener's choice in the player bar.
    shuffle: bool,
    repeat: Repeat,
    /// Bumped whenever the queue changes; published in the status.
    queue_revision: u64,
    prepared: Option<Prepared>,
    /// Last token handed to a thread opening a track ahead.
    prepare_token: u64,
}

impl Controller {
    pub fn new(
        inbox: Receiver<Inbox>,
        inbox_tx: Sender<Inbox>,
        writer: Sender<ToWriter>,
        writer_thread: Thread,
        shared: Arc<Shared>,
        on_change: Box<dyn Fn(&PlayerStatus) + Send>,
    ) -> Self {
        Self {
            inbox,
            inbox_tx,
            writer,
            writer_thread,
            free: (0..POOL_SIZE).map(|_| Vec::new()).collect(),
            shared,
            on_change,
            session: None,
            seq: 0,
            last_format: None,
            shuffle: false,
            repeat: Repeat::Off,
            queue_revision: 0,
            prepared: None,
            prepare_token: 0,
        }
    }

    pub fn run(mut self) {
        loop {
            // Blocks unless there is audio to decode and a buffer for it.
            let message = if self.decoding() && !self.free.is_empty() {
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
                Some(Inbox::Free(buffer)) => self.free.push(buffer),
                Some(Inbox::Prepared { token, result }) => self.prepared(token, result),
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
            } => self.play_queue(items, start, device, dsd),
            Command::Enqueue {
                items,
                next,
                device,
                dsd,
            } => {
                if self.session.is_some() {
                    self.edit(Edit::Add { items, next });
                } else if !items.is_empty() {
                    // Nothing playing: the tracks become the queue.
                    self.play_queue(items, 0, device, dsd);
                }
            }
            Command::Edit(edit) => self.edit(edit),
            Command::Next => {
                if let Some((queue, index, device, dsd)) = self.current() {
                    match following(index, queue.len(), self.repeat, true) {
                        Some(next) => self.begin(queue, next, 0, device, dsd),
                        None => self.stop(),
                    }
                }
            }
            Command::Previous => {
                if let Some((queue, index, device, dsd)) = self.current() {
                    let first = index == 0 && self.repeat == Repeat::Off;
                    let restart = first || self.played_seconds() >= RESTART_AFTER_SECONDS;
                    let target = match index {
                        _ if restart => index,
                        0 => queue.len() - 1,
                        _ => index - 1,
                    };
                    self.begin(queue, target, 0, device, dsd);
                }
            }
            Command::SkipTo { index, device, dsd } => {
                if let Some((queue, _, device, dsd)) = self.current() {
                    if index < queue.len() {
                        self.begin(queue, index, 0, device, dsd);
                    }
                } else if index < self.shared_queue().len() {
                    // Stopped: the stored queue plays again from `index`.
                    self.begin(
                        Queue::from_items(self.shared_queue()),
                        index,
                        0,
                        device,
                        dsd,
                    );
                }
            }
            Command::Seek { frame } => {
                // Restarting the shown track at `frame` also covers a seek
                // just after a gapless boundary, when the decoder is already
                // in the next track.
                if self.seek_in_place(frame) {
                    return;
                }
                if let Some((queue, index, device, dsd)) = self.current()
                    && self.session.as_ref().is_some_and(|s| s.output.is_some())
                {
                    self.begin(queue, index, frame, device, dsd);
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
            Command::SetShuffle(on) => self.set_shuffle(on),
            Command::SetRepeat(repeat) => self.set_repeat(repeat),
            Command::Shutdown => {}
        }
    }

    /// The queue, shown entry, device and DSD policy of the session.
    fn current(&self) -> Option<(Queue, usize, String, DsdPolicy)> {
        self.session.as_ref().map(|session| {
            (
                session.queue.clone(),
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
    fn begin(&mut self, queue: Queue, index: usize, frame: u64, device: String, dsd: DsdPolicy) {
        let Some(item) = queue.items.get(index).cloned() else {
            return self.stop();
        };
        let ready = if frame == 0 {
            self.take_prepared(&item.path)
        } else {
            None
        };
        let opened = ready.map_or_else(|| open_at(&item, frame), Ok);
        let source = match opened {
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
        self.shared.buffered.store(UNKNOWN, Ordering::Release);
        *self
            .shared
            .queue
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Arc::clone(&queue.items);
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
        let now = now_playing(&item, &entry, &device, reuse, dsd);
        let fresh = PlayerStatus {
            state,
            track: Some(now),
            ..self.fresh_status(queue.len())
        };
        self.update(|status| *status = fresh);
        self.session = Some(Session {
            queue,
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
                    let item = session.queue.items[entry.index].clone();
                    let now =
                        now_playing(&item, &entry, &session.device, session.output, session.dsd);
                    self.update(|status| status.track = Some(now));
                }
                if !paused {
                    self.update(|status| status.state = PlaybackState::Playing);
                }
            }
            WriterEvent::Drained { .. } => {
                if let Some(index) = session.after_drain {
                    // The next track needs another format: reopen for it.
                    let (queue, device, dsd) =
                        (session.queue.clone(), session.device.clone(), session.dsd);
                    self.begin(queue, index, 0, device, dsd);
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
        // None free: the writer is busy or paused, and returns one when done.
        let Some(mut buffer) = self.free.pop() else {
            return;
        };
        let feed = &mut session.feed;
        match feed.source.next(&output, session.dsd, &mut buffer) {
            Ok(Some(frames)) => {
                self.shared.conversion.publish(feed.source.conversion());
                // Near a gapless boundary the feed is ahead of the track
                // playing, which was read to its end.
                let buffered = if session.upcoming.is_empty() {
                    feed.source.buffered_frame()
                } else {
                    session.shown.info.total_frames()
                };
                self.shared
                    .buffered
                    .store(buffered.unwrap_or(UNKNOWN), Ordering::Release);
                let next = feed
                    .source
                    .fully_read()
                    .then(|| following(feed.entry.index, session.queue.len(), self.repeat, false))
                    .flatten()
                    .and_then(|next| session.queue.items.get(next))
                    .map(|item| item.path.clone());
                let chunk = Chunk {
                    generation: session.generation,
                    seq: feed.entry.seq,
                    start_frame: feed.next_frame,
                    frames,
                    bytes: buffer,
                };
                feed.next_frame += frames;
                self.to_writer(ToWriter::Chunk(chunk));
                if let Some(path) = next {
                    self.prepare(path);
                }
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
        let Some(session) = self.session.as_ref() else {
            return self.recycle(buffer);
        };
        let dsd = session.dsd;
        let after = following(
            session.feed.entry.index,
            session.queue.len(),
            self.repeat,
            false,
        );
        let next_path = after
            .and_then(|next| session.queue.items.get(next))
            .map(|item| item.path.clone());
        // A file that fails to open is reported when its turn comes.
        let opened = after.zip(next_path).and_then(|(next, path)| {
            let source = match self.take_prepared(&path) {
                Some(source) => source,
                None => Source::open(&path).ok()?,
            };
            Some((next, path, source))
        });
        let continuation = match opened {
            Some((next, _, source)) if output.carries(&source.info().stream(dsd)) => {
                Some((next, source))
            }
            Some((_, path, source)) => {
                // It needs another format: kept for its turn, after the drain.
                self.prepared = Some(Prepared::Ready {
                    path,
                    source: Box::new(source),
                });
                None
            }
            None => None,
        };
        let Some(session) = self.session.as_mut() else {
            return self.recycle(buffer);
        };

        let Some((next, mut source)) = continuation else {
            session.after_drain = after;
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

    fn play_queue(&mut self, items: Vec<QueueItem>, start: usize, device: String, dsd: DsdPolicy) {
        self.queue_revision += 1;
        let fresh = PlayerStatus {
            state: PlaybackState::Loading,
            ..self.fresh_status(items.len())
        };
        self.update(|status| *status = fresh);
        let mut queue = Queue::new(items);
        let mut start = start;
        if self.shuffle {
            // The chosen track first, then the rest in random order.
            queue = queue.shuffled(&[start]).0;
            start = 0;
        }
        self.begin(queue, start, 0, device, dsd);
    }

    fn set_shuffle(&mut self, on: bool) {
        if self.shuffle == on {
            return;
        }
        self.shuffle = on;
        if let Some(session) = &self.session {
            let (queue, moved) = if on {
                session.queue.shuffled(&session.committed())
            } else {
                session.queue.unshuffled()
            };
            self.reorder(queue, &moved);
        } else {
            self.update(|status| status.shuffle = on);
        }
    }

    /// Changes the queue. Without a session it edits the stored queue (for
    /// the Queue view), which plays when the listener picks an entry.
    fn edit(&mut self, edit: Edit) {
        let Some(session) = &self.session else {
            let stored = Queue::from_items(self.shared_queue());
            if let Some((queue, _)) = stored.edited(edit, &[], self.shuffle) {
                self.store_queue(&queue);
                self.publish_queue(queue.len(), None);
            }
            return;
        };
        if let Some((queue, moved)) = session
            .queue
            .edited(edit, &session.committed(), self.shuffle)
        {
            self.reorder(queue, &moved);
        }
    }

    /// Puts `queue` in place of the session's; `moved` maps old positions to
    /// new ones and keeps every started or decoded entry.
    fn reorder(&mut self, queue: Queue, moved: &Moved) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        let at = |index: usize| moved.get(index).copied().flatten().unwrap_or(index);
        session.queue = queue;
        session.shown.index = at(session.shown.index);
        session.feed.entry.index = at(session.feed.entry.index);
        for entry in &mut session.upcoming {
            entry.index = at(entry.index);
        }
        let (queue, shown) = (session.queue.clone(), session.shown.index);
        self.store_queue(&queue);
        self.publish_queue(queue.len(), Some(shown));
        self.plan_after_drain();
    }

    fn shared_queue(&self) -> Arc<[QueueItem]> {
        Arc::clone(
            &self
                .shared
                .queue
                .lock()
                .unwrap_or_else(PoisonError::into_inner),
        )
    }

    /// Keeps `queue` for [`crate::Player::queue`]; the status follows with
    /// [`Self::publish_queue`].
    fn store_queue(&mut self, queue: &Queue) {
        self.queue_revision += 1;
        *self
            .shared
            .queue
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Arc::clone(&queue.items);
    }

    /// One status change for a changed queue: its length and revision, the
    /// modes, and where the shown track is now.
    fn publish_queue(&self, length: usize, shown: Option<usize>) {
        let (revision, shuffle, repeat) = (self.queue_revision, self.shuffle, self.repeat);
        self.update(|status| {
            status.queue_length = length;
            status.queue_revision = revision;
            status.shuffle = shuffle;
            status.repeat = repeat;
            if let (Some(index), Some(track)) = (shown, &mut status.track) {
                track.index = index;
            }
        });
    }

    /// A stopped status that keeps the modes and the queue's revision.
    fn fresh_status(&self, queue_length: usize) -> PlayerStatus {
        PlayerStatus {
            queue_length,
            shuffle: self.shuffle,
            repeat: self.repeat,
            queue_revision: self.queue_revision,
            ..PlayerStatus::STOPPED
        }
    }

    fn set_repeat(&mut self, repeat: Repeat) {
        self.repeat = repeat;
        self.plan_after_drain();
        self.update(|status| status.repeat = repeat);
    }

    /// After a change of order or repeat while the last decoded track plays
    /// out, picks again what opens once the device drains.
    fn plan_after_drain(&mut self) {
        let repeat = self.repeat;
        if let Some(session) = self.session.as_mut()
            && !session.feed.decoding
        {
            session.after_drain =
                following(session.feed.entry.index, session.queue.len(), repeat, false);
        }
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
    fn recycle(&mut self, buffer: Vec<u8>) {
        self.free.push(buffer);
    }

    fn stop(&mut self) {
        self.close_session();
        self.shared.position.store(0, Ordering::Release);
        let fresh = self.fresh_status(self.shared_queue().len());
        self.update(|status| *status = fresh);
    }

    /// Seeks the track being decoded, when it is also the one playing: its
    /// file stays open, so a seek inside the read-ahead window costs no
    /// I/O. Otherwise (or when the seek fails) returns false, and the track
    /// is opened again, as [`Self::begin`] does.
    fn seek_in_place(&mut self, frame: u64) -> bool {
        let Some(session) = self.session.as_mut() else {
            return false;
        };
        if session.output.is_none()
            || !session.upcoming.is_empty()
            || session.feed.entry.seq != session.shown.seq
            || !session.feed.decoding
            || session.feed.source.read_to_end()
            || session.feed.source.seek(frame).is_err()
        {
            return false;
        }
        // As `begin` does for a track the open device carries.
        let generation = self.shared.next_generation();
        self.seq += 1;
        let entry = Entry {
            seq: self.seq,
            ..session.shown
        };
        session.generation = generation;
        session.started = false;
        session.shown = entry;
        session.after_drain = None;
        session.feed.entry = entry;
        session.feed.next_frame = frame;
        session.feed.decoding = true;
        let item = session.queue.items[entry.index].clone();
        let now = now_playing(&item, &entry, &session.device, session.output, session.dsd);
        let queue_length = session.queue.len();
        self.shared.paused.store(false, Ordering::Release);
        self.shared.position.store(frame, Ordering::Release);
        self.shared.buffered.store(UNKNOWN, Ordering::Release);
        // A paused writer wakes for the new generation.
        self.writer_thread.unpark();
        let fresh = PlayerStatus {
            state: self.shared.waiting_state(),
            track: Some(now),
            ..self.fresh_status(queue_length)
        };
        self.update(|status| *status = fresh);
        true
    }

    /// Opens `path` on a thread of its own, unless it is already prepared.
    fn prepare(&mut self, path: PathBuf) {
        if self
            .prepared
            .as_ref()
            .is_some_and(|prepared| prepared.path() == path)
        {
            return;
        }
        self.prepare_token += 1;
        let token = self.prepare_token;
        let inbox = self.inbox_tx.clone();
        let opening = path.clone();
        let spawned = thread::Builder::new()
            .name("lyra-prepare".into())
            .spawn(move || {
                let result = Source::open(&opening);
                // The controller is gone at shutdown: nobody needs it.
                let _ = inbox.send(Inbox::Prepared { token, result });
            });
        // Without a thread the track opens when its turn comes, as before.
        self.prepared = spawned.is_ok().then_some(Prepared::Opening { path, token });
    }

    fn prepared(&mut self, token: u64, result: Result<Source, PlayerError>) {
        let Some(Prepared::Opening {
            path,
            token: wanted,
        }) = self.prepared.take()
        else {
            return;
        };
        if token != wanted {
            self.prepared = Some(Prepared::Opening {
                path,
                token: wanted,
            });
            return;
        }
        self.prepared = Some(match result {
            Ok(source) => Prepared::Ready {
                path,
                source: Box::new(source),
            },
            Err(_) => Prepared::Failed { path },
        });
    }

    /// The prepared source for `path`, if it is ready. One still opening is
    /// dropped: the caller opens the file itself.
    fn take_prepared(&mut self, path: &Path) -> Option<Source> {
        match self.prepared.take()? {
            Prepared::Ready {
                path: ready,
                source,
            } if ready == path => Some(*source),
            _ => None,
        }
    }

    /// Invalidates queued audio and releases the device.
    fn close_session(&mut self) {
        self.prepared = None;
        if self.session.take().is_some() {
            self.shared.conversion.publish(None);
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
    dsd: DsdPolicy,
) -> NowPlaying {
    NowPlaying {
        path: item.path.clone(),
        id: item.id,
        index: entry.index,
        device: device.to_owned(),
        source: entry.info,
        output,
        dsd,
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
