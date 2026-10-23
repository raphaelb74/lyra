//! Reads a file ahead of its decoder, on a thread of its own.
//!
//! Network shares stall and disks spin up: a decoder that reads the file
//! directly passes every stall on to the device, whose queue holds well under
//! a second. Here an I/O thread keeps a window of the file in memory (about
//! 30 s of audio, capped in bytes) and the decoder reads from it. Seeks inside
//! the window cost no I/O; seeks outside restart the I/O thread there.
//!
//! Runs on the controller side only: the output thread never touches it.

use std::collections::VecDeque;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;

/// Window held until the stream's byte rate is known (headers, embedded art).
const INITIAL_LIMIT: usize = 4 << 20;
/// Audio held ahead of the decoder, once its byte rate is known.
pub(crate) const AHEAD_SECONDS: u64 = 30;
/// The window never shrinks below or grows above these.
const MIN_LIMIT: usize = 4 << 20;
const MAX_LIMIT: usize = 48 << 20;
/// Kept behind the decoder, for the short backward seeks of format probing.
const KEEP_BEHIND: u64 = 1 << 20;
/// One read from the file.
const CHUNK: usize = 256 << 10;
/// A seek this far past the window reads on to it; further, the I/O thread
/// jumps there (reading through 40 MB of DSD on a network share takes
/// seconds).
const READ_THROUGH: u64 = 1 << 20;

/// The decoder's side: `Read + Seek` over the window.
#[derive(Debug)]
pub(crate) struct ReadAhead {
    state: Arc<State>,
}

/// A view of the window, for the buffered range shown on the seek bar.
#[derive(Clone, Debug)]
pub(crate) struct Progress(Arc<State>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Window {
    /// File length when opened.
    pub len: u64,
    /// Where the decoder reads.
    pub pos: u64,
    /// End of the bytes in memory.
    pub end: u64,
    /// Every byte from `pos` to the end of the file is in memory.
    pub complete: bool,
}

#[derive(Debug)]
struct State {
    inner: Mutex<Inner>,
    changed: Condvar,
}

#[derive(Debug)]
struct Inner {
    /// File offset of `bytes[0]`.
    start: u64,
    bytes: VecDeque<u8>,
    /// Bytes held ahead of `pos`.
    limit: usize,
    /// Where the decoder reads.
    pos: u64,
    len: u64,
    /// The I/O thread is to continue from here, dropping the window.
    restart: Option<u64>,
    eof: bool,
    error: Option<(io::ErrorKind, String)>,
    /// The decoder is gone: the I/O thread stops.
    closed: bool,
}

impl Inner {
    fn end(&self) -> u64 {
        self.start + self.bytes.len() as u64
    }

    fn ahead(&self) -> u64 {
        self.end().saturating_sub(self.pos)
    }

    /// The I/O thread has room to read (or something to do).
    fn wants_io(&self) -> bool {
        self.closed
            || self.restart.is_some()
            || (!self.eof && self.error.is_none() && self.ahead() < self.limit as u64)
    }

    /// Drops what lies further behind the decoder than [`KEEP_BEHIND`].
    fn trim(&mut self) {
        let keep_from = self.pos.saturating_sub(KEEP_BEHIND);
        if keep_from > self.start {
            let drop = usize::try_from(keep_from - self.start)
                .unwrap_or(usize::MAX)
                .min(self.bytes.len());
            self.bytes.drain(..drop);
            self.start += drop as u64;
        }
    }
}

impl State {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn wait<'a>(&self, guard: MutexGuard<'a, Inner>) -> MutexGuard<'a, Inner> {
        self.changed
            .wait(guard)
            .unwrap_or_else(PoisonError::into_inner)
    }
}

impl ReadAhead {
    /// Opens `path` and starts reading it on a new thread.
    pub fn open(path: &Path) -> io::Result<(Self, Progress)> {
        let file = File::open(path)?;
        let len = file.metadata()?.len();
        let state = Arc::new(State {
            inner: Mutex::new(Inner {
                start: 0,
                bytes: VecDeque::new(),
                limit: INITIAL_LIMIT,
                pos: 0,
                len,
                restart: None,
                eof: false,
                error: None,
                closed: false,
            }),
            changed: Condvar::new(),
        });
        let io_state = Arc::clone(&state);
        thread::Builder::new()
            .name("lyra-read".into())
            .spawn(move || read_file(file, &io_state))?;
        Ok((
            Self {
                state: Arc::clone(&state),
            },
            Progress(state),
        ))
    }
}

impl Read for ReadAhead {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let mut inner = self.state.lock();
        loop {
            let (pos, end) = (inner.pos, inner.end());
            if (inner.start..end).contains(&pos) {
                let offset = usize::try_from(pos - inner.start).unwrap_or(usize::MAX);
                let (front, back) = inner.bytes.as_slices();
                let available = if offset < front.len() {
                    &front[offset..]
                } else {
                    &back[offset - front.len()..]
                };
                let n = available.len().min(buf.len());
                buf[..n].copy_from_slice(&available[..n]);
                inner.pos += n as u64;
                inner.trim();
                let room = inner.wants_io();
                drop(inner);
                if room {
                    self.state.changed.notify_all();
                }
                return Ok(n);
            }
            if pos >= inner.len && inner.restart.is_none() {
                return Ok(0);
            }
            let reachable = pos >= inner.start && pos <= end + READ_THROUGH;
            if inner.restart.is_none() && !reachable {
                inner.restart = Some(pos);
            } else if inner.restart.is_none() && pos >= end {
                if let Some((kind, message)) = &inner.error {
                    return Err(io::Error::new(*kind, message.clone()));
                }
                if inner.eof {
                    // The file shrank since it was opened.
                    return Ok(0);
                }
            }
            // A seek may have given the I/O thread something to do (a new
            // place to read, or room past a full window) while it sleeps.
            if inner.wants_io() {
                self.state.changed.notify_all();
            }
            inner = self.state.wait(inner);
        }
    }
}

impl Seek for ReadAhead {
    /// Only moves the position: reads fetch what is not in memory.
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let mut inner = self.state.lock();
        let target = match pos {
            SeekFrom::Start(offset) => Some(offset),
            SeekFrom::Current(delta) => inner.pos.checked_add_signed(delta),
            SeekFrom::End(delta) => inner.len.checked_add_signed(delta),
        };
        inner.pos = target.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "seek before the start of the file",
            )
        })?;
        Ok(inner.pos)
    }
}

impl Drop for ReadAhead {
    fn drop(&mut self) {
        self.state.lock().closed = true;
        self.state.changed.notify_all();
    }
}

impl Progress {
    pub fn window(&self) -> Window {
        let inner = self.0.lock();
        let end = inner.end();
        Window {
            len: inner.len,
            pos: inner.pos,
            end,
            complete: end >= inner.len && inner.pos >= inner.start,
        }
    }

    /// Holds about [`AHEAD_SECONDS`] of a stream of `bytes_per_second`.
    pub fn set_rate(&self, bytes_per_second: u64) {
        let wanted = bytes_per_second.saturating_mul(AHEAD_SECONDS);
        let limit = usize::try_from(wanted)
            .unwrap_or(usize::MAX)
            .clamp(MIN_LIMIT, MAX_LIMIT);
        self.0.lock().limit = limit;
        self.0.changed.notify_all();
    }
}

/// The I/O thread: fills the window until the decoder is gone.
fn read_file(mut file: File, state: &State) {
    let mut chunk = vec![0; CHUNK];
    let mut file_pos = 0;
    loop {
        let mut inner = state.lock();
        while !inner.wants_io() {
            inner = state.wait(inner);
        }
        if inner.closed {
            return;
        }
        if let Some(at) = inner.restart.take() {
            inner.start = at;
            inner.bytes.clear();
            inner.eof = false;
            inner.error = None;
            if let Err(error) = file.seek(SeekFrom::Start(at)) {
                inner.error = Some((error.kind(), error.to_string()));
                drop(inner);
                state.changed.notify_all();
                continue;
            }
            file_pos = at;
        }
        let room = (inner.limit as u64).saturating_sub(inner.ahead());
        let want = usize::try_from(room).unwrap_or(CHUNK).clamp(1, CHUNK);
        drop(inner);

        let read = loop {
            match file.read(&mut chunk[..want]) {
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                result => break result,
            }
        };

        let mut inner = state.lock();
        if inner.restart.is_some() || inner.closed {
            // The decoder moved elsewhere meanwhile: this read is stale.
            continue;
        }
        match read {
            Ok(0) => inner.eof = true,
            Ok(n) => {
                inner.bytes.extend(&chunk[..n]);
                file_pos += n as u64;
            }
            Err(error) => inner.error = Some((error.kind(), error.to_string())),
        }
        debug_assert_eq!(file_pos, inner.end());
        drop(inner);
        state.changed.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// A file of its own per test; left for the OS to clean up.
    fn temp_file(test: &str, bytes: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("unda-readahead-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create test folder");
        let path = dir.join(test);
        std::fs::write(&path, bytes).expect("write test file");
        path
    }

    /// A pattern where nearby offsets differ.
    #[expect(clippy::cast_possible_truncation, reason = "wraps on purpose")]
    fn pattern(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i * 7 + i / 251) as u8).collect()
    }

    fn wait_until(progress: &Progress, done: impl Fn(Window) -> bool) -> Window {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let window = progress.window();
            if done(window) {
                return window;
            }
            assert!(
                Instant::now() < deadline,
                "window never got there: {window:?}"
            );
            thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn reads_the_whole_file_unchanged() {
        let data = pattern(3 * CHUNK + 1234);
        let path = temp_file("reads_the_whole_file_unchanged", &data);
        let (mut reader, progress) = ReadAhead::open(&path).expect("open");
        let mut out = Vec::new();
        reader.read_to_end(&mut out).expect("read");
        assert_eq!(out, data);
        assert!(progress.window().complete);
    }

    #[test]
    fn seeks_inside_and_outside_the_window() {
        let data = pattern(MIN_LIMIT * 3);
        let path = temp_file("seeks_inside_and_outside_the_window", &data);
        let (mut reader, _progress) = ReadAhead::open(&path).expect("open");
        let mut buf = [0; 4096];
        for at in [100, 50, MIN_LIMIT * 2 + 17, 10, data.len() - 4096, 0] {
            reader.seek(SeekFrom::Start(at as u64)).expect("seek");
            reader.read_exact(&mut buf).expect("read");
            assert_eq!(buf[..], data[at..at + 4096], "at {at}");
        }
        reader.seek(SeekFrom::End(-10)).expect("seek from end");
        let mut tail = Vec::new();
        reader.read_to_end(&mut tail).expect("read tail");
        assert_eq!(tail, data[data.len() - 10..]);
        assert!(
            reader
                .seek(SeekFrom::Current(-i64::try_from(data.len()).unwrap() - 1))
                .is_err()
        );
    }

    #[test]
    fn holds_no_more_than_the_limit_ahead() {
        let data = pattern(MIN_LIMIT * 4);
        let path = temp_file("holds_no_more_than_the_limit_ahead", &data);
        let (mut reader, progress) = ReadAhead::open(&path).expect("open");
        let mut buf = [0; 16];
        reader.read_exact(&mut buf).expect("read");
        let window = wait_until(&progress, |w| {
            w.end - w.pos >= INITIAL_LIMIT as u64 - CHUNK as u64
        });
        thread::sleep(Duration::from_millis(50));
        assert!(
            progress.window().end - window.pos <= INITIAL_LIMIT as u64,
            "{window:?}"
        );
        assert!(!progress.window().complete);

        // Reading on frees room, and the window follows.
        reader
            .seek(SeekFrom::Start(MIN_LIMIT as u64))
            .expect("seek");
        reader.read_exact(&mut buf).expect("read");
        wait_until(&progress, |w| w.end >= 2 * MIN_LIMIT as u64);
    }

    #[test]
    fn a_larger_rate_reads_further_ahead() {
        let data = pattern(MIN_LIMIT * 3);
        let path = temp_file("a_larger_rate_reads_further_ahead", &data);
        let (mut reader, progress) = ReadAhead::open(&path).expect("open");
        let mut buf = [0; 16];
        reader.read_exact(&mut buf).expect("read");
        // 10 MiB ahead: 30 s at 350 kB/s.
        progress.set_rate(350_000);
        let window = wait_until(&progress, |w| w.end >= 10_000_000);
        assert!(window.end - window.pos <= 30 * 350_000);
    }

    /// Random seeks (all kinds) and reads of random sizes match the file.
    #[test]
    fn random_access_matches_the_file() {
        let data = pattern(MIN_LIMIT * 2 + 12_345);
        let path = temp_file("random_access_matches_the_file", &data);
        let (mut reader, _progress) = ReadAhead::open(&path).expect("open");
        let len = data.len() as u64;
        let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut next = |bound: u64| {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            (seed >> 33) % bound.max(1)
        };
        let mut pos = 0_u64;
        let mut buf = vec![0; 100_000];
        for step in 0..2_000 {
            match next(4) {
                0 => pos = reader.seek(SeekFrom::Start(next(len + 10))).expect("seek"),
                1 => {
                    let back = i64::try_from(next(pos.min(300_000) + 1)).unwrap();
                    pos = reader.seek(SeekFrom::Current(-back)).expect("seek back");
                }
                2 => {
                    let back = i64::try_from(next(len) + 1).unwrap();
                    pos = reader.seek(SeekFrom::End(-back)).expect("seek from end");
                }
                _ => {}
            }
            let want = usize::try_from(next(buf.len() as u64) + 1).unwrap();
            let n = reader.read(&mut buf[..want]).expect("read");
            let start = usize::try_from(pos).unwrap().min(data.len());
            let expected = &data[start..(start + want).min(data.len())];
            assert!(
                n > 0 || expected.is_empty(),
                "step {step}: 0 bytes at {pos}"
            );
            assert_eq!(&buf[..n], &expected[..n], "step {step} at {pos}");
            pos += n as u64;
        }
    }

    /// A seek just past a full window must wake the I/O thread, or the
    /// reader waits for it forever.
    #[test]
    fn a_seek_past_a_full_window_still_reads() {
        let data = pattern(INITIAL_LIMIT * 3);
        let path = temp_file("a_seek_past_a_full_window_still_reads", &data);
        let (mut reader, progress) = ReadAhead::open(&path).expect("open");
        let mut buf = [0; 16];
        reader.read_exact(&mut buf).expect("read");
        wait_until(&progress, |w| w.end - w.pos >= INITIAL_LIMIT as u64);
        // Let the I/O thread go to sleep on the full window.
        thread::sleep(Duration::from_millis(100));
        let at = progress.window().end + 1000;
        reader.seek(SeekFrom::Start(at)).expect("seek");
        reader.read_exact(&mut buf).expect("read past the window");
        let at = usize::try_from(at).unwrap();
        assert_eq!(buf[..], data[at..at + 16]);
    }

    #[test]
    fn end_of_file_reads_zero() {
        let data = pattern(100);
        let path = temp_file("end_of_file_reads_zero", &data);
        let (mut reader, _progress) = ReadAhead::open(&path).expect("open");
        reader.seek(SeekFrom::Start(500)).expect("seek past end");
        assert_eq!(reader.read(&mut [0; 8]).expect("read"), 0);
    }
}
