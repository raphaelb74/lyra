//! A local fake server and sample MusicBrainz answers, shared by the tests.
#![allow(
    dead_code,
    clippy::unwrap_used,
    reason = "each test file uses some helpers; helpers fail loudly"
)]

use std::fmt::Write as _;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use musicbrainz::{Client, Service};
use serde_json::json;

pub const RELEASE: &str = "b10bbbfc-cf9e-42e0-be17-e2c3e1d2600d";

/// What the fake server does with one connection.
pub enum Reply {
    Send(Vec<u8>),
    /// Accept, read the request, then say nothing for this long.
    Hang(Duration),
}

/// A server answering one request per connection, in order, and keeping each
/// request's head for the test to inspect.
pub struct Server {
    pub addr: String,
    requests: Arc<Mutex<Vec<String>>>,
}

impl Server {
    pub fn start(replies: Vec<Reply>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&requests);
        thread::spawn(move || {
            for reply in replies {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                seen.lock().unwrap().push(read_head(&mut stream));
                match reply {
                    // The client may stop reading early; that is the point of some tests.
                    Reply::Send(bytes) => drop(stream.write_all(&bytes)),
                    Reply::Hang(time) => thread::sleep(time),
                }
            }
        });
        Self { addr, requests }
    }

    pub fn service(&self) -> Service {
        Service {
            musicbrainz: format!("http://{}/ws/2/", self.addr),
            cover_art: format!("http://{}/", self.addr),
            interval: Duration::from_millis(50),
            backoff: Duration::from_millis(20),
            timeout: Duration::from_secs(5),
        }
    }

    pub fn client(&self) -> Client {
        Client::new(self.service(), "1.2.3")
    }

    pub fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }
}

fn read_head(stream: &mut TcpStream) -> String {
    let mut head = Vec::new();
    let mut byte = [0];
    while !head.ends_with(b"\r\n\r\n") && stream.read(&mut byte).unwrap_or(0) == 1 {
        head.push(byte[0]);
    }
    String::from_utf8_lossy(&head).into_owned()
}

pub fn reply(status: &str, headers: &[(&str, &str)], body: &[u8]) -> Reply {
    let mut bytes = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    for (name, value) in headers {
        write!(bytes, "{name}: {value}\r\n").unwrap();
    }
    bytes.push_str("\r\n");
    let mut bytes = bytes.into_bytes();
    bytes.extend_from_slice(body);
    Reply::Send(bytes)
}

pub fn json_reply(value: &serde_json::Value) -> Reply {
    reply(
        "200 OK",
        &[("Content-Type", "application/json")],
        value.to_string().as_bytes(),
    )
}

pub fn search_page() -> serde_json::Value {
    json!({
        "created": "2026-10-23T09:00:00.000Z",
        "count": 2,
        "offset": 0,
        "releases": [
            {
                "id": RELEASE,
                "score": 100,
                "title": "Kind of Blue",
                "status": "Official",
                "date": "1959-08-17",
                "country": "US",
                "track-count": 5,
                "artist-credit": [{ "name": "Miles Davis", "artist": { "id": "561d854a-6a28-4aa7-8c99-323e6ce46c2a", "name": "Miles Davis", "sort-name": "Davis, Miles" } }],
                "label-info": [{ "catalog-number": "CL 1355", "label": { "name": "Columbia" } }],
                "media": [{ "format": "12\" Vinyl", "track-count": 5 }]
            },
            { "id": "0bbbcd4b-4e7a-4b5a-8d1b-1b0f2d1e3c4a", "score": "87", "title": "Kind of Blue", "date": null }
        ]
    })
}

pub fn release_lookup() -> serde_json::Value {
    let credit = |name: &str, join: &str| json!({ "name": name, "joinphrase": join, "artist": { "id": "561d854a-6a28-4aa7-8c99-323e6ce46c2a", "name": name, "sort-name": name } });
    json!({
        "id": RELEASE,
        "title": "Kind of Blue",
        "disambiguation": "remastered",
        "date": "1997",
        "artist-credit": [credit("Miles Davis", "")],
        "release-group": { "id": "8a7bda59-4a7b-3a5e-a3b8-4fd6e2a4bd8d", "primary-type": "Album", "secondary-types": [], "first-release-date": "1959-08-17" },
        "label-info": [],
        "media": [
            { "position": 1, "format": "CD", "track-count": 2, "tracks": [
                { "id": "11111111-1111-4111-8111-111111111111", "number": "1", "position": 1, "title": "So What", "length": 562_000,
                  "artist-credit": [credit("Miles Davis", "")], "recording": { "id": "21111111-1111-4111-8111-111111111111", "title": "So What", "length": 562_000 } },
                { "id": "11111111-1111-4111-8111-111111111112", "number": "2", "position": 2, "title": "Freddie Freeloader", "length": null,
                  "artist-credit": [credit("Miles Davis", " feat. "), credit("Wynton Kelly", "")], "recording": { "id": "21111111-1111-4111-8111-111111111112", "title": "Freddie Freeloader" } }
            ] },
            { "position": 2, "format": "CD", "track-count": 0, "tracks": [] }
        ]
    })
}
