use std::collections::HashMap;
use std::io::Read;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use serde::de::DeserializeOwned;
use ureq::http::{Response, Uri};
use ureq::{Agent, Body};

use crate::types::SearchPage;
use crate::{Error, Release, is_mbid};

/// The project's contact address, sent in the User-Agent as MusicBrainz asks.
/// It names the project, never the person using Lyra.
pub const CONTACT: &str = "lyra-player.doorman433@slmails.com";

/// Largest JSON answer, before and after decompression.
const JSON_LIMIT: u64 = 4 * 1024 * 1024;
/// Largest cover image: the same cap as a cover chosen from a file.
const IMAGE_LIMIT: u64 = 16 * 1024 * 1024;
/// Cover Art Archive redirects to archive.org, which redirects to a data node.
const MAX_REDIRECTS: usize = 5;
/// Further tries after MusicBrainz refuses a request for load.
const RETRIES: u32 = 3;
/// Longest wait a `Retry-After` header may ask for.
const MAX_RETRY_AFTER: Duration = Duration::from_secs(60);
/// Answers kept for the session, so the same question is not asked twice.
const CACHE_ENTRIES: usize = 64;
const RELEASE_INCLUDES: &str = "recordings+artist-credits+release-groups+labels+media";

/// Where requests go and how they are paced.
#[derive(Clone, Debug)]
pub struct Service {
    /// MusicBrainz API root, ending in `/`.
    pub musicbrainz: String,
    /// Cover Art Archive root, ending in `/`.
    pub cover_art: String,
    /// Least time between two MusicBrainz requests, across all callers.
    pub interval: Duration,
    /// First wait after MusicBrainz refuses a request for load; it doubles on
    /// each retry.
    pub backoff: Duration,
    /// Longest a request may take, from connecting to the last byte.
    pub timeout: Duration,
}

impl Service {
    /// The public services, at one MusicBrainz request per second.
    #[must_use]
    pub fn official() -> Self {
        Self {
            musicbrainz: "https://musicbrainz.org/ws/2/".to_owned(),
            cover_art: "https://coverartarchive.org/".to_owned(),
            interval: Duration::from_secs(1),
            backoff: Duration::from_secs(2),
            timeout: Duration::from_secs(30),
        }
    }
}

/// A shared client: one per app, used from any number of worker threads.
#[derive(Debug)]
pub struct Client {
    agent: Agent,
    service: Service,
    limiter: Limiter,
    cache: Mutex<HashMap<String, Vec<u8>>>,
}

impl Client {
    /// `app_version` goes into the User-Agent: `Lyra/<version> ( <contact> )`.
    #[must_use]
    pub fn new(service: Service, app_version: &str) -> Self {
        let config = Agent::config_builder()
            .user_agent(format!("Lyra/{app_version} ( {CONTACT} )"))
            // Statuses are handled here, and redirects are followed by hand so
            // each hop's host is checked.
            .http_status_as_error(false)
            .max_redirects(0)
            .timeout_global(Some(service.timeout))
            .timeout_connect(Some(service.timeout.min(Duration::from_secs(10))))
            .build();
        Self {
            agent: Agent::new_with_config(config),
            limiter: Limiter::new(service.interval),
            service,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// Releases matching a [Lucene query](https://musicbrainz.org/doc/MusicBrainz_API/Search),
    /// best first. `limit` is clamped to 1–100.
    pub fn search_releases(&self, query: &str, limit: u8) -> Result<Vec<Release>, Error> {
        let url = format!("{}release/", self.service.musicbrainz);
        let limit = limit.clamp(1, 100).to_string();
        let page: SearchPage =
            parse(&self.musicbrainz(&url, &[("query", query), ("limit", &limit)])?)?;
        Ok(page.releases)
    }

    /// One release with its discs, tracks, credits, labels and release group.
    pub fn release(&self, id: &str) -> Result<Release, Error> {
        if !is_mbid(id) {
            return Err(Error::BadId);
        }
        let url = format!("{}release/{id}", self.service.musicbrainz);
        parse(&self.musicbrainz(&url, &[("inc", RELEASE_INCLUDES)])?)
    }

    /// The release's front cover, about 1200 pixels wide, else about 500; `None`
    /// when the release has none. The bytes are not checked as an image here.
    pub fn front_cover(&self, release_id: &str) -> Result<Option<Vec<u8>>, Error> {
        if !is_mbid(release_id) {
            return Err(Error::BadId);
        }
        for size in ["1200", "500"] {
            let url = format!(
                "{}release/{release_id}/front-{size}",
                self.service.cover_art
            );
            match self.image(&url) {
                Ok(bytes) => return Ok(Some(bytes)),
                Err(Error::NotFound) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(None)
    }

    /// A MusicBrainz GET through the limiter and the cache, retried with a
    /// growing wait while the service refuses it for load.
    fn musicbrainz(&self, url: &str, query: &[(&str, &str)]) -> Result<Vec<u8>, Error> {
        let key = query.iter().fold(url.to_owned(), |key, (name, value)| {
            format!("{key}\n{name}={value}")
        });
        if let Some(body) = lock(&self.cache).get(&key) {
            return Ok(body.clone());
        }
        let mut wait = self.service.backoff;
        let mut attempt = 0;
        loop {
            self.limiter.wait();
            let mut request = self.agent.get(url).header("Accept", "application/json");
            for (name, value) in query {
                // `inc` joins its parts with a literal `+`.
                request = if name == &"inc" {
                    request.query_raw(name, value)
                } else {
                    request.query(name, value)
                };
            }
            let mut response = request.query("fmt", "json").call()?;
            match response.status().as_u16() {
                200 => {
                    let body = read(response.body_mut(), JSON_LIMIT)?;
                    let mut cache = lock(&self.cache);
                    if cache.len() >= CACHE_ENTRIES {
                        cache.clear();
                    }
                    cache.insert(key, body.clone());
                    return Ok(body);
                }
                404 => return Err(Error::NotFound),
                429 | 503 if attempt < RETRIES => {
                    self.limiter
                        .defer(retry_after(&response).map_or(wait, |asked| asked.max(wait)));
                    wait *= 2;
                    attempt += 1;
                }
                429 | 503 => return Err(Error::Busy),
                status => return Err(Error::Status(status)),
            }
        }
    }

    /// A Cover Art Archive GET, following redirects only to the archive's own
    /// hosts with the same scheme.
    fn image(&self, url: &str) -> Result<Vec<u8>, Error> {
        let mut url = url.to_owned();
        for _ in 0..=MAX_REDIRECTS {
            let mut response = self.agent.get(&url).call()?;
            match response.status().as_u16() {
                200 => return read(response.body_mut(), IMAGE_LIMIT),
                301 | 302 | 303 | 307 | 308 => url = self.redirect(&url, &response)?,
                404 => return Err(Error::NotFound),
                429 | 503 => return Err(Error::Busy),
                status => return Err(Error::Status(status)),
            }
        }
        Err(Error::Redirect("too many redirects".to_owned()))
    }

    fn redirect(&self, from: &str, response: &Response<Body>) -> Result<String, Error> {
        let location = response
            .headers()
            .get("location")
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| Error::Invalid("a redirect without a location".to_owned()))?;
        redirect_target(&self.service.cover_art, from, location)
    }
}

/// Where a cover redirect leads, if Lyra follows it: the same scheme as the
/// Cover Art Archive root, to its host or to `archive.org` and its subdomains.
fn redirect_target(cover_art: &str, from: &str, location: &str) -> Result<String, Error> {
    let bad = || Error::Redirect("a bad address".to_owned());
    let to = if location.starts_with('/') {
        let from: Uri = from.parse().map_err(|_| bad())?;
        let (Some(scheme), Some(authority)) = (from.scheme_str(), from.authority()) else {
            return Err(bad());
        };
        format!("{scheme}://{authority}{location}")
    } else {
        location.to_owned()
    };
    let uri: Uri = to.parse().map_err(|_| bad())?;
    let base: Uri = cover_art.parse().map_err(|_| bad())?;
    let host = uri.host().unwrap_or_default().to_ascii_lowercase();
    let allowed = uri.scheme_str().is_some()
        && uri.scheme_str() == base.scheme_str()
        && (base
            .host()
            .is_some_and(|base| base.eq_ignore_ascii_case(&host))
            || host == "archive.org"
            || host.ends_with(".archive.org"));
    if allowed {
        Ok(to)
    } else {
        Err(Error::Redirect(format!("to {host}")))
    }
}

/// Reads a body of at most `limit` bytes, counted both on the wire and after
/// decompression, since a small gzip body can expand without bound.
fn read(body: &mut Body, limit: u64) -> Result<Vec<u8>, Error> {
    let mut bytes = Vec::new();
    body.with_config()
        .limit(limit)
        .reader()
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(Error::TooLarge);
    }
    Ok(bytes)
}

fn parse<T: DeserializeOwned>(body: &[u8]) -> Result<T, Error> {
    serde_json::from_slice(body).map_err(|error| Error::Invalid(error.to_string()))
}

/// A `Retry-After` given in seconds, capped.
fn retry_after(response: &Response<Body>) -> Option<Duration> {
    let seconds: u64 = response
        .headers()
        .get("retry-after")?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()?;
    Some(Duration::from_secs(seconds).min(MAX_RETRY_AFTER))
}

/// The cache and the limiter hold plain values that are valid at every step,
/// so a panic elsewhere cannot leave them inconsistent.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Spaces requests at least `interval` apart, across threads.
#[derive(Debug)]
struct Limiter {
    interval: Duration,
    /// The earliest time the next request may start.
    next: Mutex<Option<Instant>>,
}

impl Limiter {
    fn new(interval: Duration) -> Self {
        Self {
            interval,
            next: Mutex::new(None),
        }
    }

    /// Waits for this request's turn. The turn is taken under the lock and the
    /// wait happens outside it, so waiting callers queue in order.
    fn wait(&self) {
        let delay = {
            let mut next = lock(&self.next);
            let now = Instant::now();
            let start = next.map_or(now, |next| next.max(now));
            *next = Some(start + self.interval);
            start - now
        };
        if !delay.is_zero() {
            thread::sleep(delay);
        }
    }

    /// Holds every request back for at least `delay` from now.
    fn defer(&self, delay: Duration) {
        let mut next = lock(&self.next);
        let until = Instant::now() + delay;
        *next = Some(next.map_or(until, |next| next.max(until)));
    }
}

#[cfg(test)]
mod tests {
    use super::{Error, redirect_target};

    const ROOT: &str = "https://coverartarchive.org/";
    const FROM: &str = "https://coverartarchive.org/release/x/front-1200";

    #[test]
    fn the_archive_hosts_are_followed_over_https() {
        for location in [
            "https://archive.org/download/mbid-x/front.jpg",
            "https://ia800.us.archive.org/1/items/mbid-x/front.jpg",
            "https://CoverArtArchive.org/release/x/front",
        ] {
            assert_eq!(redirect_target(ROOT, FROM, location).unwrap(), location);
        }
        assert_eq!(
            redirect_target(ROOT, FROM, "/release/x/front").unwrap(),
            "https://coverartarchive.org/release/x/front"
        );
    }

    #[test]
    fn other_hosts_and_downgrades_are_refused() {
        for location in [
            "http://archive.org/download/mbid-x/front.jpg",
            "https://evilarchive.org/front.jpg",
            "https://archive.org.example.com/front.jpg",
            "https://example.com/front.jpg",
            "ftp://archive.org/front.jpg",
            "archive.org/front.jpg",
            "not a url",
        ] {
            assert!(
                matches!(
                    redirect_target(ROOT, FROM, location),
                    Err(Error::Redirect(_))
                ),
                "{location}"
            );
        }
    }
}
