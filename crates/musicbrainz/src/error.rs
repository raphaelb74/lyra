use std::fmt;
use std::io;

/// Why a request gave no answer. Messages are written for the listener.
#[derive(Debug)]
pub enum Error {
    /// The service could not be reached: no network, no DNS, refused, TLS.
    Unreachable(String),
    /// The service took too long to answer.
    Timeout,
    /// The service is busy or limiting Lyra, and still was after waiting.
    Busy,
    /// No such release, or no such image.
    NotFound,
    /// An HTTP status Lyra does not expect.
    Status(u16),
    /// A response larger than Lyra accepts.
    TooLarge,
    /// A redirect Lyra does not follow: another host, another scheme, or too many.
    Redirect(String),
    /// A response Lyra could not read.
    Invalid(String),
    /// Not a MusicBrainz ID; nothing was sent.
    BadId,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreachable(reason) => write!(f, "Could not connect ({reason})."),
            Self::Timeout => f.write_str("The server took too long to answer."),
            Self::Busy => f.write_str("The server is busy. Try again in a minute."),
            Self::NotFound => f.write_str("Not found."),
            Self::Status(status) => write!(f, "The server answered with an error ({status})."),
            Self::TooLarge => f.write_str("The answer was larger than Lyra accepts."),
            Self::Redirect(reason) => write!(f, "Lyra did not follow a redirect ({reason})."),
            Self::Invalid(reason) => write!(f, "Lyra could not read the answer ({reason})."),
            Self::BadId => f.write_str("That is not a MusicBrainz ID."),
        }
    }
}

impl std::error::Error for Error {}

impl From<ureq::Error> for Error {
    fn from(error: ureq::Error) -> Self {
        match error {
            ureq::Error::Timeout(_) => Self::Timeout,
            ureq::Error::BodyExceedsLimit(_) => Self::TooLarge,
            ureq::Error::Io(error) if error.kind() == io::ErrorKind::TimedOut => Self::Timeout,
            other => Self::Unreachable(other.to_string()),
        }
    }
}

impl From<io::Error> for Error {
    /// A read error from a response body, which may wrap a `ureq` error.
    fn from(error: io::Error) -> Self {
        ureq::Error::from(error).into()
    }
}
