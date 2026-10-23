//! The client against a local fake server: no test goes online.
#![allow(clippy::unwrap_used, reason = "helpers fail loudly")]

use std::fmt::Write as _;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use musicbrainz::{CONTACT, Client, Error, Service};
use serde_json::json;

const RELEASE: &str = "b10bbbfc-cf9e-42e0-be17-e2c3e1d2600d";

/// What the fake server does with one connection.
enum Reply {
    Send(Vec<u8>),
    /// Accept, read the request, then say nothing for this long.
    Hang(Duration),
}

/// A server answering one request per connection, in order, and keeping each
/// request's head for the test to inspect.
struct Server {
    addr: String,
    requests: Arc<Mutex<Vec<String>>>,
}

impl Server {
    fn start(replies: Vec<Reply>) -> Self {
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

    fn service(&self) -> Service {
        Service {
            musicbrainz: format!("http://{}/ws/2/", self.addr),
            cover_art: format!("http://{}/", self.addr),
            interval: Duration::from_millis(50),
            backoff: Duration::from_millis(20),
            timeout: Duration::from_secs(5),
        }
    }

    fn client(&self) -> Client {
        Client::new(self.service(), "1.2.3")
    }

    fn requests(&self) -> Vec<String> {
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

fn reply(status: &str, headers: &[(&str, &str)], body: &[u8]) -> Reply {
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

fn json_reply(value: &serde_json::Value) -> Reply {
    reply(
        "200 OK",
        &[("Content-Type", "application/json")],
        value.to_string().as_bytes(),
    )
}

fn search_page() -> serde_json::Value {
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

fn release_lookup() -> serde_json::Value {
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

#[test]
fn search_names_lyra_and_sends_nothing_personal() {
    let server = Server::start(vec![json_reply(&search_page())]);
    let releases = server
        .client()
        .search_releases("release:\"Kind of Blue\" AND artist:Miles", 10)
        .unwrap();

    assert_eq!(releases.len(), 2);
    assert_eq!(releases[0].score, Some(100));
    assert_eq!(
        releases[1].score,
        Some(87),
        "a score sent as text is read too"
    );
    assert_eq!(
        releases[0].artist_credit[0].artist.sort_name,
        "Davis, Miles"
    );
    assert_eq!(
        releases[0].label_info[0].catalog_number.as_deref(),
        Some("CL 1355")
    );
    assert_eq!(releases[1].date, None);

    let requests = server.requests();
    let head = requests[0].to_lowercase();
    let line = head.lines().next().unwrap();
    assert!(line.starts_with("get /ws/2/release/?"), "{line}");
    assert!(
        line.contains("fmt=json") && line.contains("limit=10"),
        "{line}"
    );
    assert!(
        !line.contains('"') && !line.contains("kind of blue"),
        "the query is encoded: {line}"
    );
    assert!(
        head.contains(&format!("user-agent: lyra/1.2.3 ( {CONTACT} )")),
        "{head}"
    );
    assert!(head.contains("accept: application/json"), "{head}");
    for header in ["cookie:", "authorization:", "referer:"] {
        assert!(!head.contains(header), "{header} sent: {head}");
    }
}

#[test]
fn release_lookup_reads_discs_tracks_and_credits() {
    let server = Server::start(vec![json_reply(&release_lookup())]);
    let release = server.client().release(RELEASE).unwrap();

    assert_eq!(release.disambiguation.as_deref(), Some("remastered"));
    let group = release.release_group.unwrap();
    assert_eq!(group.first_release_date.as_deref(), Some("1959-08-17"));
    assert_eq!(release.media.len(), 2);
    let tracks = &release.media[0].tracks;
    assert_eq!(tracks[0].length, Some(562_000));
    assert_eq!(tracks[1].length, None);
    assert_eq!(tracks[1].artist_credit[0].joinphrase, " feat. ");
    assert_eq!(
        tracks[1].recording.id,
        "21111111-1111-4111-8111-111111111112"
    );

    let line = server.requests()[0].lines().next().unwrap().to_owned();
    assert!(
        line.starts_with(&format!("GET /ws/2/release/{RELEASE}?")),
        "{line}"
    );
    assert!(
        line.contains("inc=recordings+artist-credits+release-groups+labels+media"),
        "{line}"
    );
}

#[test]
fn ids_are_checked_before_anything_is_sent() {
    let server = Server::start(Vec::new());
    let client = server.client();
    for id in ["../search", "B10BBBFC-CF9E-42E0-BE17-E2C3E1D2600D", ""] {
        assert!(matches!(client.release(id), Err(Error::BadId)));
        assert!(matches!(client.front_cover(id), Err(Error::BadId)));
    }
    assert_eq!(server.requests().len(), 0);
}

#[test]
fn requests_are_spaced_and_answers_reused() {
    let server = Server::start(vec![
        json_reply(&search_page()),
        json_reply(&search_page()),
        json_reply(&search_page()),
    ]);
    let client = server.client();
    let start = Instant::now();
    for query in ["a", "b", "c", "a", "b"] {
        client.search_releases(query, 5).unwrap();
    }
    assert_eq!(
        server.requests().len(),
        3,
        "repeated questions come from memory"
    );
    assert!(
        start.elapsed() >= Duration::from_millis(100),
        "{:?}",
        start.elapsed()
    );
}

#[test]
fn the_limiter_is_shared_across_threads() {
    let server = Server::start((0..4).map(|_| json_reply(&search_page())).collect());
    let client = Arc::new(server.client());
    let start = Instant::now();
    let workers: Vec<_> = ["a", "b", "c", "d"]
        .into_iter()
        .map(|query| {
            let client = Arc::clone(&client);
            thread::spawn(move || client.search_releases(query, 5).map(|_| ()))
        })
        .collect();
    for worker in workers {
        worker.join().unwrap().unwrap();
    }
    assert!(
        start.elapsed() >= Duration::from_millis(150),
        "{:?}",
        start.elapsed()
    );
}

#[test]
fn a_refusal_for_load_waits_then_retries() {
    let server = Server::start(vec![
        reply("503 Service Unavailable", &[], b""),
        reply("429 Too Many Requests", &[], b""),
        json_reply(&release_lookup()),
    ]);
    let start = Instant::now();
    server.client().release(RELEASE).unwrap();
    // 20 ms, then 40 ms.
    assert!(
        start.elapsed() >= Duration::from_millis(60),
        "{:?}",
        start.elapsed()
    );
    assert_eq!(server.requests().len(), 3);
}

#[test]
fn retry_after_is_honoured() {
    let server = Server::start(vec![
        reply("503 Service Unavailable", &[("Retry-After", "1")], b""),
        json_reply(&release_lookup()),
    ]);
    let start = Instant::now();
    server.client().release(RELEASE).unwrap();
    assert!(
        start.elapsed() >= Duration::from_secs(1),
        "{:?}",
        start.elapsed()
    );
}

#[test]
fn lasting_refusals_give_busy() {
    let server = Server::start(
        (0..5)
            .map(|_| reply("503 Service Unavailable", &[], b""))
            .collect(),
    );
    assert!(matches!(server.client().release(RELEASE), Err(Error::Busy)));
    assert_eq!(server.requests().len(), 4, "one try and three retries");
}

#[test]
fn statuses_are_reported() {
    let server = Server::start(vec![
        reply("404 Not Found", &[], b""),
        reply("500 Internal Server Error", &[], b""),
        reply("200 OK", &[], b"<html>"),
    ]);
    let client = server.client();
    assert!(matches!(client.release(RELEASE), Err(Error::NotFound)));
    assert!(matches!(
        client.search_releases("x", 1),
        Err(Error::Status(500))
    ));
    assert!(matches!(
        client.search_releases("y", 1),
        Err(Error::Invalid(_))
    ));
}

#[test]
fn oversized_answers_are_refused() {
    let big = vec![b' '; 4 * 1024 * 1024 + 1];
    let server = Server::start(vec![reply("200 OK", &[], &big)]);
    assert!(matches!(
        server.client().search_releases("x", 1),
        Err(Error::TooLarge)
    ));
}

#[test]
fn compressed_answers_are_limited_after_decompression() {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    encoder.write_all(&vec![b' '; 8 * 1024 * 1024]).unwrap();
    let bomb = encoder.finish().unwrap();
    assert!(bomb.len() < 64 * 1024);
    let server = Server::start(vec![reply(
        "200 OK",
        &[("Content-Encoding", "gzip")],
        &bomb,
    )]);
    assert!(matches!(
        server.client().search_releases("x", 1),
        Err(Error::TooLarge)
    ));
}

#[test]
fn a_silent_server_times_out() {
    let server = Server::start(vec![Reply::Hang(Duration::from_secs(2))]);
    let service = Service {
        timeout: Duration::from_millis(300),
        ..server.service()
    };
    let start = Instant::now();
    assert!(matches!(
        Client::new(service, "1").release(RELEASE),
        Err(Error::Timeout)
    ));
    assert!(start.elapsed() < Duration::from_secs(2));
}

#[test]
fn a_closed_port_is_unreachable() {
    let addr = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();
    let service = Service {
        musicbrainz: format!("http://{addr}/ws/2/"),
        ..Service::official()
    };
    assert!(matches!(
        Client::new(service, "1").release(RELEASE),
        Err(Error::Unreachable(_))
    ));
}

#[test]
fn covers_follow_redirects_on_the_archive_only() {
    let image = b"\xff\xd8\xff\xe0 not really a jpeg";
    let server = Server::start(vec![
        reply(
            "307 Temporary Redirect",
            &[("Location", "/images/front.jpg")],
            b"",
        ),
        reply("200 OK", &[("Content-Type", "image/jpeg")], image),
    ]);
    assert_eq!(
        server.client().front_cover(RELEASE).unwrap().as_deref(),
        Some(&image[..])
    );
    let requests = server.requests();
    assert!(requests[0].starts_with(&format!("GET /release/{RELEASE}/front-1200 ")));
    assert!(requests[1].starts_with("GET /images/front.jpg "));
}

#[test]
fn covers_refuse_other_hosts_and_schemes() {
    let server = Server::start(Vec::new());
    let addr = server.addr.clone();
    drop(server);
    for location in [
        "http://example.com/front.jpg".to_owned(),
        // Same host, other scheme: no switching between http and https.
        format!("https://{addr}/front.jpg"),
        "https://ia800.us.archive.org/front.jpg".to_owned(),
    ] {
        let server = Server::start(vec![reply("302 Found", &[("Location", &location)], b"")]);
        let result = server.client().front_cover(RELEASE);
        assert!(
            matches!(result, Err(Error::Redirect(_))),
            "{location}: {result:?}"
        );
        assert_eq!(server.requests().len(), 1, "{location} was not requested");
    }
}

#[test]
fn redirect_loops_stop() {
    let server = Server::start(
        (0..10)
            .map(|_| reply("307 Temporary Redirect", &[("Location", "/again")], b""))
            .collect(),
    );
    assert!(matches!(
        server.client().front_cover(RELEASE),
        Err(Error::Redirect(_))
    ));
    assert_eq!(server.requests().len(), 6);
}

#[test]
fn a_release_without_a_cover_gives_none() {
    let server = Server::start(vec![
        reply("404 Not Found", &[], b""),
        reply("404 Not Found", &[], b""),
    ]);
    assert_eq!(server.client().front_cover(RELEASE).unwrap(), None);
    let requests = server.requests();
    assert!(requests[0].contains("/front-1200 "));
    assert!(requests[1].contains("/front-500 "));
}
