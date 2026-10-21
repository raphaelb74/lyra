//! Taking a card from the sound server through device reservation.
//!
//! Sound servers (WirePlumber, PulseAudio) own the D-Bus name
//! `org.freedesktop.ReserveDevice1.Audio<card>` for each card they manage and
//! hand the card over when a program with a higher priority asks for it
//! (<https://git.0pointer.net/reserve.git/tree/reserve.txt>). Lyra asks only
//! when the card is busy, and holds the name until it closes the device; the
//! sound server takes the card back when the name is free again.

use std::fmt;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use dbus::blocking::Connection;
use dbus::blocking::stdintf::org_freedesktop_dbus::RequestNameReply;
use dbus::channel::{MatchingReceiver, Sender as _};
use dbus::message::MatchRule;
use dbus::strings::{BusName, Path};

/// Above WirePlumber's default (-20) and PulseAudio's (0).
const PRIORITY: i32 = 10;
/// Longest wait for the owner to answer a release request.
const TIMEOUT: Duration = Duration::from_secs(3);
/// How often the bus thread checks whether the claim was dropped.
const POLL: Duration = Duration::from_millis(100);
const INTERFACE: &str = "org.freedesktop.ReserveDevice1";

/// Lyra's claim on one card.
///
/// A thread serves the bus while the claim lives: the sound server asks for
/// the card back at once, and only notices the card is free again if that
/// request was answered. Dropping the claim stops the thread and closes the
/// connection, which frees the name.
pub(crate) struct Reservation {
    card: i32,
    /// Dropped first: its closing tells the thread to stop.
    stop: Option<Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl Reservation {
    /// Asks the current owner of `card` to hand it over.
    ///
    /// `None` when there is no session bus, no owner to ask (the card is held
    /// by a program outside the protocol), or the owner refused.
    pub(crate) fn acquire(card: i32) -> Option<Self> {
        Self::acquire_on(card, Connection::new_session().ok()?)
    }

    fn acquire_on(card: i32, bus: Connection) -> Option<Self> {
        let name = BusName::new(name(card)).ok()?;
        let path = Path::new(format!("/org/freedesktop/ReserveDevice1/Audio{card}")).ok()?;
        // Free name: nobody to ask, so whoever holds the card ignores the
        // protocol. Dropping the connection gives the name back.
        if owned(bus.request_name(name.clone(), false, false, true).ok()?) {
            return None;
        }
        let (released,): (bool,) = bus
            .with_proxy(name.clone(), path, TIMEOUT)
            .method_call(INTERFACE, "RequestRelease", (PRIORITY,))
            .ok()?;
        // Replacing the released owner, not allowing anyone to replace Lyra.
        if !(released && owned(bus.request_name(name, false, true, true).ok()?)) {
            return None;
        }
        // While playing, Lyra keeps the card: every release request is refused.
        bus.start_receive(
            MatchRule::new_method_call()
                .with_interface(INTERFACE)
                .with_member("RequestRelease"),
            Box::new(|call, bus| {
                let _ = bus.send(call.method_return().append1(false));
                true
            }),
        );
        let (stop, stopped) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("lyra-reserve".into())
            .spawn(move || serve(&bus, &stopped))
            .ok()?;
        Some(Self {
            card,
            stop: Some(stop),
            thread: Some(thread),
        })
    }

    pub(crate) fn card(&self) -> i32 {
        self.card
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        self.stop = None;
        // Joined so the name is free before the sink reports the card closed.
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl fmt::Debug for Reservation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Reservation")
            .field("card", &self.card)
            .finish_non_exhaustive()
    }
}

/// Answers bus calls until the claim is dropped; the connection closes on return.
fn serve(bus: &Connection, stopped: &Receiver<()>) {
    while matches!(stopped.try_recv(), Err(TryRecvError::Empty)) {
        if bus.process(POLL).is_err() {
            return;
        }
    }
}

fn name(card: i32) -> String {
    format!("{INTERFACE}.Audio{card}")
}

fn owned(reply: RequestNameReply) -> bool {
    matches!(
        reply,
        RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner
    )
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader};
    use std::process::{Child, Command, Stdio};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

    use dbus::channel::Channel;

    use super::*;

    const CARD: i32 = 7;

    /// A private session bus, so tests never touch the desktop's sound server.
    struct Bus {
        daemon: Child,
        address: String,
    }

    impl Bus {
        fn start() -> Self {
            let mut daemon = Command::new("dbus-daemon")
                .args(["--session", "--nofork", "--print-address=1"])
                .stdout(Stdio::piped())
                .spawn()
                .expect("dbus-daemon must be installed to run these tests");
            let mut address = String::new();
            BufReader::new(daemon.stdout.take().unwrap())
                .read_line(&mut address)
                .unwrap();
            Self {
                daemon,
                address: address.trim().to_owned(),
            }
        }

        fn connect(&self) -> Connection {
            let mut channel = Channel::open_private(&self.address).unwrap();
            channel.register().unwrap();
            Connection::from(channel)
        }

        fn has_owner(&self) -> bool {
            let (owned,): (bool,) = self
                .connect()
                .with_proxy("org.freedesktop.DBus", "/org/freedesktop/DBus", TIMEOUT)
                .method_call("org.freedesktop.DBus", "NameHasOwner", (name(CARD),))
                .unwrap();
            owned
        }

        /// Asks whoever owns the card to release it, as a sound server would.
        fn request_release(&self, priority: i32) -> bool {
            let (released,): (bool,) = self
                .connect()
                .with_proxy(
                    name(CARD),
                    format!("/org/freedesktop/ReserveDevice1/Audio{CARD}"),
                    TIMEOUT,
                )
                .method_call(INTERFACE, "RequestRelease", (priority,))
                .unwrap();
            released
        }
    }

    impl Drop for Bus {
        fn drop(&mut self) {
            let _ = self.daemon.kill();
            let _ = self.daemon.wait();
        }
    }

    /// A sound server owning the card, like WirePlumber: replaceable, and
    /// answering release requests with `release`.
    struct Server {
        asked: Arc<AtomicI32>,
        stop: Arc<AtomicBool>,
        thread: Option<JoinHandle<()>>,
    }

    impl Server {
        fn start(bus: &Bus, release: bool) -> Self {
            let connection = bus.connect();
            let reply = connection
                .request_name(name(CARD), true, false, true)
                .unwrap();
            assert!(owned(reply));
            let asked = Arc::new(AtomicI32::new(i32::MIN));
            let stop = Arc::new(AtomicBool::new(false));
            let seen = Arc::clone(&asked);
            connection.start_receive(
                MatchRule::new_method_call().with_member("RequestRelease"),
                Box::new(move |call, connection| {
                    seen.store(call.read1().unwrap(), Ordering::SeqCst);
                    let _ = connection.send(call.method_return().append1(release));
                    true
                }),
            );
            let stopping = Arc::clone(&stop);
            let thread = thread::spawn(move || {
                while !stopping.load(Ordering::SeqCst) {
                    connection.process(POLL).unwrap();
                }
            });
            Self {
                asked,
                stop,
                thread: Some(thread),
            }
        }
    }

    impl Drop for Server {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
            if let Some(thread) = self.thread.take() {
                thread.join().unwrap();
            }
        }
    }

    #[test]
    fn takes_the_card_keeps_it_and_gives_it_back() {
        let bus = Bus::start();
        let server = Server::start(&bus, true);

        let reservation = Reservation::acquire_on(CARD, bus.connect()).unwrap();
        assert_eq!(server.asked.load(Ordering::SeqCst), PRIORITY);
        assert_eq!(reservation.card(), CARD);
        // The server asks for the card back at once: refused while held.
        assert!(!bus.request_release(-20));

        drop(reservation);
        assert!(
            !bus.has_owner(),
            "the name is free for the server to take back"
        );
    }

    #[test]
    fn a_refusing_owner_keeps_the_card() {
        let bus = Bus::start();
        let server = Server::start(&bus, false);

        assert!(Reservation::acquire_on(CARD, bus.connect()).is_none());
        assert_eq!(server.asked.load(Ordering::SeqCst), PRIORITY);
        assert!(bus.has_owner());
    }

    #[test]
    fn a_free_name_is_not_kept() {
        let bus = Bus::start();

        // Nobody to ask: whatever holds the card ignores the protocol.
        assert!(Reservation::acquire_on(CARD, bus.connect()).is_none());
        assert!(!bus.has_owner());
    }
}
