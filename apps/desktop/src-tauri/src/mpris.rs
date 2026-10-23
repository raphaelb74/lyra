//! MPRIS: the desktop's media keys (play/pause, next, previous, stop), its
//! sound menu and tools such as `playerctl` control Lyra over the session
//! bus, and show what is playing.
//!
//! One thread owns the connection. It sleeps until a message arrives or the
//! player's status changes, answers method calls, and tells listeners which
//! properties changed.
//! Without a session bus Lyra simply has no media-key support.

use std::io;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use dbus::arg::{PropMap, RefArg, Variant};
use dbus::blocking::Connection;
use dbus::blocking::stdintf::org_freedesktop_dbus::RequestNameReply;
use dbus::channel::{BusType, Channel};
use dbus::channel::{MatchingReceiver, Sender as _};
use dbus::message::MatchRule;
use dbus::strings::{Interface, Member, Path};
use dbus::{Message, MethodErr};
use tauri::{AppHandle, Manager};

use crate::commands::AppState;
use crate::ipc::{ART_URL, IpcError, Status};
use audio_alsa::Wake;

const NAME: &str = "org.mpris.MediaPlayer2.lyra";
const OBJECT: &str = "/org/mpris/MediaPlayer2";
const ROOT: &str = "org.mpris.MediaPlayer2";
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
const INTROSPECTABLE: &str = "org.freedesktop.DBus.Introspectable";
const PEER: &str = "org.freedesktop.DBus.Peer";
const NO_TRACK: &str = "/org/mpris/MediaPlayer2/TrackList/NoTrack";
const MICROS: f64 = 1_000_000.0;

/// Serves MPRIS while the app runs. `changes` wakes it when the player's
/// status changed.
pub fn spawn(handle: AppHandle, changes: Arc<Wake>) -> io::Result<()> {
    thread::Builder::new()
        .name("lyra-mpris".into())
        .spawn(move || serve(&handle, &changes))
        .map(drop)
}

fn serve(handle: &AppHandle, changes: &Wake) {
    let Ok(mut channel) = Channel::get_private(BusType::Session) else {
        return;
    };
    // Exposes the connection's descriptor, to sleep on it.
    channel.set_watch_enabled(true);
    let bus = Connection::from(channel);
    // A second Lyra takes a per-process name, as the spec allows.
    let owned = |name: &str| {
        matches!(
            bus.request_name(name, false, false, true),
            Ok(RequestNameReply::PrimaryOwner)
        )
    };
    if !owned(NAME) && !owned(&format!("{NAME}.instance{}", std::process::id())) {
        return;
    }
    let calls = handle.clone();
    bus.start_receive(
        MatchRule::new_method_call(),
        Box::new(move |call, bus| {
            for message in answer(&calls, &call) {
                let _ = bus.send(message);
            }
            true
        }),
    );

    let mut shown = Snapshot::read(handle, None);
    loop {
        // Everything received so far, including what libdbus read while
        // sending; then the replies go out.
        loop {
            match bus.process(Duration::ZERO) {
                Ok(true) => {}
                Ok(false) => break,
                Err(_) => return,
            }
        }
        bus.channel().flush();
        changes.wait(Some(bus.channel().watch().fd), None);
        if changes.drain()
            && let Some(now) = Snapshot::read(handle, shown.as_ref())
        {
            let props = now.changed_since(shown.as_ref());
            if !props.is_empty() {
                let _ = bus.send(properties_changed(props));
            }
            shown = Some(now);
        }
    }
}

/// What MPRIS shows, read from the player and the library.
#[derive(Clone, Debug, PartialEq)]
struct Snapshot {
    playback: &'static str,
    can_next: bool,
    can_play: bool,
    can_seek: bool,
    track: Option<TrackMeta>,
}

#[derive(Clone, Debug, PartialEq)]
struct TrackMeta {
    id: String,
    /// The library data below is looked up again only when this changes.
    library_id: Option<i64>,
    title: String,
    artist: Option<String>,
    album: Option<String>,
    art_url: Option<String>,
    length: Option<i64>,
}

impl Snapshot {
    /// The player's state now; library data comes from `shown` when the
    /// track is the same.
    fn read(handle: &AppHandle, shown: Option<&Self>) -> Option<Self> {
        let state = handle.try_state::<AppState>()?;
        Some(Self::from_status(
            &Status::from(&state.player.status()),
            &state,
            shown,
        ))
    }

    fn from_status(status: &Status, state: &AppState, shown: Option<&Self>) -> Self {
        let known = shown.and_then(|shown| shown.track.as_ref());
        let track = status.track.as_ref().map(|track| {
            let id = format!("/audio/lyra/track/{}", track.queue_index);
            let length = track.duration_seconds.map(seconds_to_micros);
            if let Some(known) =
                known.filter(|k| k.library_id.is_some() && k.library_id == track.library_id)
            {
                return TrackMeta {
                    id,
                    length,
                    ..known.clone()
                };
            }
            let details = track
                .library_id
                .and_then(|id| state.track_details(id).ok().flatten());
            let art_url = details
                .as_ref()
                .and_then(|details| details.art.as_deref())
                .and_then(|url| url.strip_prefix(ART_URL))
                .map(|name| format!("file://{}", state.art_dir.join(name).display()));
            TrackMeta {
                id,
                library_id: track.library_id,
                title: details
                    .as_ref()
                    .map_or_else(|| track.file_name.clone(), |d| d.title.clone()),
                artist: details.as_ref().map(|d| d.artist.clone()),
                album: details.and_then(|d| d.album),
                art_url,
                length,
            }
        });
        Self {
            playback: playback(status.state, track.is_some()),
            can_next: status.has_next,
            can_play: track.is_some(),
            can_seek: track.as_ref().is_some_and(|t| t.length.is_some()),
            track,
        }
    }

    fn metadata(&self) -> PropMap {
        let mut map = PropMap::new();
        let Some(track) = &self.track else {
            map.insert("mpris:trackid".into(), value(object_path(NO_TRACK)));
            return map;
        };
        map.insert("mpris:trackid".into(), value(object_path(&track.id)));
        map.insert("xesam:title".into(), value(track.title.clone()));
        if let Some(artist) = &track.artist {
            map.insert("xesam:artist".into(), value(vec![artist.clone()]));
        }
        if let Some(album) = &track.album {
            map.insert("xesam:album".into(), value(album.clone()));
        }
        if let Some(url) = &track.art_url {
            map.insert("mpris:artUrl".into(), value(url.clone()));
        }
        if let Some(length) = track.length {
            map.insert("mpris:length".into(), value(length));
        }
        map
    }

    /// Player properties that differ from `before` (all when unknown).
    fn changed_since(&self, before: Option<&Self>) -> PropMap {
        let mut props = PropMap::new();
        let differs = |pick: fn(&Self) -> String| before.is_none_or(|b| pick(b) != pick(self));
        if differs(|s| s.playback.to_owned()) {
            props.insert("PlaybackStatus".into(), value(self.playback.to_owned()));
        }
        if before.is_none_or(|b| b.track != self.track) {
            props.insert("Metadata".into(), value(self.metadata()));
        }
        if before.is_none_or(|b| b.can_next != self.can_next) {
            props.insert("CanGoNext".into(), value(self.can_next));
        }
        if before.is_none_or(|b| b.can_play != self.can_play) {
            props.insert("CanPlay".into(), value(self.can_play));
            props.insert("CanPause".into(), value(self.can_play));
            props.insert("CanGoPrevious".into(), value(self.can_play));
        }
        if before.is_none_or(|b| b.can_seek != self.can_seek) {
            props.insert("CanSeek".into(), value(self.can_seek));
        }
        props
    }
}

/// MPRIS playback status of a Lyra player state.
fn playback(state: &str, has_track: bool) -> &'static str {
    match state {
        "paused" => "Paused",
        "playing" | "loading" | "negotiating" | "buffering" if has_track => "Playing",
        _ => "Stopped",
    }
}

/// Replies (and signals) for one method call.
fn answer(handle: &AppHandle, call: &Message) -> Vec<Message> {
    let interface = call.interface().map(|i| i.to_string()).unwrap_or_default();
    let member = call.member().map(|m| m.to_string()).unwrap_or_default();
    match (interface.as_str(), member.as_str()) {
        (PEER, "Ping") => return vec![call.method_return()],
        (INTROSPECTABLE, "Introspect") => {
            return vec![call.method_return().append1(INTROSPECTION)];
        }
        _ => {}
    }
    if call.path().is_none_or(|path| &*path != OBJECT) {
        return vec![MethodErr::no_method(&member).to_message(call)];
    }
    let Some(state) = handle.try_state::<AppState>() else {
        return vec![MethodErr::failed("Lyra is starting").to_message(call)];
    };
    let player = &state.player;
    let done = |result: Result<(), unda::PlayerError>| match result {
        Ok(()) => vec![call.method_return()],
        Err(error) => vec![MethodErr::failed(&IpcError::from(error).message).to_message(call)],
    };
    match (interface.as_str(), member.as_str()) {
        (PROPERTIES, "Get") => match call.read2::<&str, &str>() {
            Ok((interface, name)) => match properties(&state, interface).remove(name) {
                Some(found) => vec![call.method_return().append1(found)],
                None => vec![MethodErr::no_property(name).to_message(call)],
            },
            Err(error) => vec![MethodErr::from(error).to_message(call)],
        },
        (PROPERTIES, "GetAll") => match call.read1::<&str>() {
            Ok(interface) => vec![call.method_return().append1(properties(&state, interface))],
            Err(error) => vec![MethodErr::from(error).to_message(call)],
        },
        (PROPERTIES, "Set") => vec![MethodErr::ro_property(&"property").to_message(call)],
        (ROOT, "Raise") => {
            if let Some(window) = handle.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
            vec![call.method_return()]
        }
        (ROOT, "Quit") => vec![call.method_return()],
        (PLAYER, "PlayPause") => {
            let playing = Status::from(&player.status()).state == "playing";
            done(if playing {
                player.pause()
            } else {
                player.resume()
            })
        }
        (PLAYER, "Play") => done(player.resume()),
        (PLAYER, "Pause") => done(player.pause()),
        (PLAYER, "Stop") => done(player.stop()),
        (PLAYER, "Next") => done(player.next()),
        (PLAYER, "Previous") => done(player.previous()),
        (PLAYER, "Seek") => match call.read1::<i64>() {
            Ok(offset) => seek_to(
                call,
                &state,
                state.position().seconds + micros_to_seconds(offset),
            ),
            Err(error) => vec![MethodErr::from(error).to_message(call)],
        },
        (PLAYER, "SetPosition") => match call.read2::<Path, i64>() {
            Ok((track, at)) => {
                let current = Snapshot::from_status(&Status::from(&player.status()), &state, None);
                if current.track.is_some_and(|t| *track == *t.id) {
                    seek_to(call, &state, micros_to_seconds(at))
                } else {
                    // The spec: a stale track id is ignored.
                    vec![call.method_return()]
                }
            }
            Err(error) => vec![MethodErr::from(error).to_message(call)],
        },
        _ => vec![MethodErr::no_method(&member).to_message(call)],
    }
}

/// Seeks and announces the new position, as MPRIS asks.
fn seek_to(call: &Message, state: &AppState, seconds: f64) -> Vec<Message> {
    let seconds = seconds.max(0.0);
    match state.seek(seconds) {
        Ok(()) => vec![
            call.method_return(),
            Message::signal(
                &object_path(OBJECT),
                &Interface::from(PLAYER),
                &Member::from("Seeked"),
            )
            .append1(seconds_to_micros(seconds)),
        ],
        Err(error) => vec![MethodErr::failed(&error.message).to_message(call)],
    }
}

/// Every property of `interface`, read now.
fn properties(state: &AppState, interface: &str) -> PropMap {
    let mut props = PropMap::new();
    match interface {
        ROOT => {
            props.insert("CanQuit".into(), value(false));
            props.insert("CanRaise".into(), value(true));
            props.insert("HasTrackList".into(), value(false));
            props.insert("Identity".into(), value("Lyra".to_owned()));
            props.insert("DesktopEntry".into(), value("Lyra".to_owned()));
            props.insert("SupportedUriSchemes".into(), value(Vec::<String>::new()));
            props.insert("SupportedMimeTypes".into(), value(Vec::<String>::new()));
        }
        PLAYER => {
            let snapshot =
                Snapshot::from_status(&Status::from(&state.player.status()), state, None);
            props = snapshot.changed_since(None);
            props.insert(
                "Position".into(),
                value(seconds_to_micros(state.position().seconds)),
            );
            props.insert("Rate".into(), value(1.0_f64));
            props.insert("MinimumRate".into(), value(1.0_f64));
            props.insert("MaximumRate".into(), value(1.0_f64));
            props.insert("CanControl".into(), value(true));
        }
        _ => {}
    }
    props
}

fn properties_changed(changed: PropMap) -> Message {
    Message::signal(
        &object_path(OBJECT),
        &Interface::from(PROPERTIES),
        &Member::from("PropertiesChanged"),
    )
    .append3(PLAYER, changed, Vec::<String>::new())
}

fn value<T: RefArg + 'static>(inner: T) -> Variant<Box<dyn RefArg>> {
    Variant(Box::new(inner))
}

/// An object path; MPRIS's "no track" path if `path` is not a valid one.
fn object_path(path: &str) -> Path<'static> {
    Path::new(path.to_owned()).unwrap_or_else(|_| Path::from(NO_TRACK))
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "track lengths and positions are far below i64::MAX microseconds"
)]
fn seconds_to_micros(seconds: f64) -> i64 {
    (seconds * MICROS).round() as i64
}

#[expect(
    clippy::cast_precision_loss,
    reason = "microsecond offsets fit an f64 exactly up to 285 years"
)]
fn micros_to_seconds(micros: i64) -> f64 {
    micros as f64 / MICROS
}

const INTROSPECTION: &str = r#"<!DOCTYPE node PUBLIC "-//freedesktop//DTD D-BUS Object Introspection 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/introspect.dtd">
<node>
  <interface name="org.mpris.MediaPlayer2">
    <method name="Raise"/>
    <method name="Quit"/>
    <property name="CanQuit" type="b" access="read"/>
    <property name="CanRaise" type="b" access="read"/>
    <property name="HasTrackList" type="b" access="read"/>
    <property name="Identity" type="s" access="read"/>
    <property name="DesktopEntry" type="s" access="read"/>
    <property name="SupportedUriSchemes" type="as" access="read"/>
    <property name="SupportedMimeTypes" type="as" access="read"/>
  </interface>
  <interface name="org.mpris.MediaPlayer2.Player">
    <method name="Next"/>
    <method name="Previous"/>
    <method name="Pause"/>
    <method name="PlayPause"/>
    <method name="Stop"/>
    <method name="Play"/>
    <method name="Seek"><arg name="Offset" type="x" direction="in"/></method>
    <method name="SetPosition">
      <arg name="TrackId" type="o" direction="in"/>
      <arg name="Position" type="x" direction="in"/>
    </method>
    <signal name="Seeked"><arg name="Position" type="x"/></signal>
    <property name="PlaybackStatus" type="s" access="read"/>
    <property name="Rate" type="d" access="read"/>
    <property name="Metadata" type="a{sv}" access="read"/>
    <property name="Position" type="x" access="read"/>
    <property name="MinimumRate" type="d" access="read"/>
    <property name="MaximumRate" type="d" access="read"/>
    <property name="CanGoNext" type="b" access="read"/>
    <property name="CanGoPrevious" type="b" access="read"/>
    <property name="CanPlay" type="b" access="read"/>
    <property name="CanPause" type="b" access="read"/>
    <property name="CanSeek" type="b" access="read"/>
    <property name="CanControl" type="b" access="read"/>
  </interface>
  <interface name="org.freedesktop.DBus.Properties">
    <method name="Get">
      <arg name="interface" type="s" direction="in"/>
      <arg name="property" type="s" direction="in"/>
      <arg name="value" type="v" direction="out"/>
    </method>
    <method name="GetAll">
      <arg name="interface" type="s" direction="in"/>
      <arg name="properties" type="a{sv}" direction="out"/>
    </method>
    <signal name="PropertiesChanged">
      <arg name="interface" type="s"/>
      <arg name="changed" type="a{sv}"/>
      <arg name="invalidated" type="as"/>
    </signal>
  </interface>
</node>"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lyra_states_map_to_mpris_status() {
        assert_eq!(playback("playing", true), "Playing");
        assert_eq!(playback("buffering", true), "Playing");
        assert_eq!(playback("paused", true), "Paused");
        assert_eq!(playback("stopped", false), "Stopped");
        assert_eq!(playback("error", true), "Stopped");
        assert_eq!(playback("playing", false), "Stopped");
    }

    #[test]
    fn microseconds_round_trip() {
        assert_eq!(seconds_to_micros(1.5), 1_500_000);
        assert!((micros_to_seconds(2_250_000) - 2.25).abs() < f64::EPSILON);
    }

    #[test]
    fn only_changed_player_properties_are_announced() {
        let stopped = Snapshot {
            playback: "Stopped",
            can_next: false,
            can_play: false,
            can_seek: false,
            track: None,
        };
        let playing = Snapshot {
            playback: "Playing",
            ..stopped.clone()
        };
        let mut changed: Vec<_> = playing.changed_since(Some(&stopped)).into_keys().collect();
        changed.sort();
        assert_eq!(changed, ["PlaybackStatus"]);
        assert!(stopped.changed_since(None).contains_key("Metadata"));
    }
}
