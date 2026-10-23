//! Wakes a watcher thread that sleeps in poll(2), instead of having it look
//! for news on a timer.
//!
//! A connected socket pair: [`Wake::notify`] writes a byte, the watcher sleeps
//! until its end is readable, then [`Wake::drain`]s it. Many notifications
//! before the watcher runs count as one.

use std::io::{self, Read, Write};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, RawFd};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use alsa::poll::{Flags, pollfd};

#[derive(Debug)]
pub struct Wake {
    notify: UnixStream,
    watch: UnixStream,
}

impl Wake {
    pub fn new() -> io::Result<Self> {
        let (notify, watch) = UnixStream::pair()?;
        notify.set_nonblocking(true)?;
        watch.set_nonblocking(true)?;
        Ok(Self { notify, watch })
    }

    /// Wakes the watcher. Never blocks: when the socket is full, a wake is
    /// already pending.
    pub fn notify(&self) {
        let _ = (&self.notify).write(&[1]);
    }

    /// Readable while a wake is pending.
    pub fn fd(&self) -> BorrowedFd<'_> {
        self.watch.as_fd()
    }

    /// Clears pending wakes; call before looking at what changed. Returns
    /// whether there were any.
    pub fn drain(&self) -> bool {
        let mut bytes = [0; 64];
        let mut woken = false;
        while matches!((&self.watch).read(&mut bytes), Ok(n) if n > 0) {
            woken = true;
        }
        woken
    }

    /// Sleeps until woken, `other` is readable, or `timeout` passes (never,
    /// with `None`). Returns whether `other` is readable.
    ///
    /// `other` is a raw descriptor (a D-Bus connection's); a stale one only
    /// makes poll(2) report it invalid.
    pub fn wait(&self, other: Option<RawFd>, timeout: Option<Duration>) -> bool {
        let watched = |fd: RawFd| pollfd {
            fd,
            events: Flags::IN.bits(),
            revents: 0,
        };
        let mut fds: Vec<pollfd> = std::iter::once(self.fd().as_raw_fd())
            .chain(other)
            .map(watched)
            .collect();
        let millis = timeout.map_or(-1, |timeout| {
            i32::try_from(timeout.as_millis()).unwrap_or(i32::MAX)
        });
        // An error (interrupted) reads as a wake: the caller looks again.
        alsa::poll::poll(&mut fds, millis).is_ok() && fds.get(1).is_some_and(|fd| fd.revents != 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;
    use std::time::Instant;

    #[test]
    fn a_notify_ends_the_wait_and_drain_resets_it() {
        let wake = Arc::new(Wake::new().unwrap());
        let notifier = Arc::clone(&wake);
        let started = Instant::now();
        let handle = thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            notifier.notify();
            notifier.notify();
        });
        assert!(!wake.wait(None, Some(Duration::from_secs(10))));
        assert!(started.elapsed() < Duration::from_secs(5));
        handle.join().unwrap();

        assert!(wake.drain());
        assert!(!wake.drain());
        let started = Instant::now();
        wake.wait(None, Some(Duration::from_millis(50)));
        assert!(
            started.elapsed() >= Duration::from_millis(40),
            "nothing pending after drain"
        );
    }

    #[test]
    fn reports_the_other_descriptor() {
        let wake = Wake::new().unwrap();
        let (mut writer, reader) = UnixStream::pair().unwrap();
        assert!(!wake.wait(Some(reader.as_raw_fd()), Some(Duration::from_millis(10))));
        writer.write_all(b"x").unwrap();
        assert!(wake.wait(Some(reader.as_raw_fd()), Some(Duration::from_secs(10))));
    }
}
