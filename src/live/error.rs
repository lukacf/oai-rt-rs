use std::{fmt, sync::Arc};

/// A snapshot contradicted the transport's bound session, or arrived before a
/// primary's first valid `session.started`. Payloads are explicit-only.
#[derive(Clone, PartialEq, Eq)]
pub struct SessionIdentityMismatch {
    /// `None` means the primary has not bound its first started session.
    pub expected_session_id: Option<String>,
    pub observed_session_id: String,
    pub raw: serde_json::Value,
}

impl fmt::Debug for SessionIdentityMismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SessionIdentityMismatch { [payload redacted] }")
    }
}

/// Public Live failures. Display/Debug deliberately omit server bodies, URLs,
/// credentials, transcripts, and function arguments.
pub enum Error {
    Invalid(String),
    Json(serde_json::Error),
    /// A known event failed decoding. Raw data is explicit-only and never printed
    /// by Display/Debug; applications can inspect it without inventing success.
    MalformedEvent {
        raw: serde_json::Value,
        source: serde_json::Error,
    },
    Http {
        status: u16,
        headers: Box<reqwest::header::HeaderMap>,
        request_id: Option<String>,
        body: Vec<u8>,
        content_type: Option<String>,
        retry_after: Option<String>,
        body_issue: Option<HttpBodyIssue>,
    },
    Transport(String),
    Provider(super::ProviderError),
    Timeout,
    /// Delivery may have occurred. The library never retries an ambiguous write.
    AmbiguousWrite,
    /// The connection ended without a `session.closed` final usage event.
    UnconfirmedClose,
    /// Sticky identity failure. The offending snapshot is not an accepted event
    /// or final-usage receipt. No further commands may start on this transport.
    SessionIdentityMismatch(Arc<SessionIdentityMismatch>),
    /// The transport could not retain a complete frame within its configured bound.
    ContinuityLost,
    Closed,
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(reason) => write!(f, "invalid Live request: {reason}"),
            Self::Json(_) => f.write_str("invalid Live JSON/schema"),
            Self::MalformedEvent { .. } => {
                f.write_str("malformed Live event; raw payload available explicitly")
            }
            Self::Http { status, .. } => {
                write!(f, "Live HTTP error ({status}); body available explicitly")
            }
            Self::Transport(_) => f.write_str("Live transport failed"),
            Self::Provider(_) => {
                f.write_str("Live provider rejected a request; details available explicitly")
            }
            Self::Timeout => f.write_str("Live operation timed out"),
            Self::AmbiguousWrite => {
                f.write_str("Live write delivery is unconfirmed; do not blindly retry")
            }
            Self::UnconfirmedClose => {
                f.write_str("Live connection ended without final usage confirmation")
            }
            Self::SessionIdentityMismatch(_) => f.write_str(
                "Live session identity mismatch; remote close and final usage unconfirmed",
            ),
            Self::ContinuityLost => {
                f.write_str("Live frame capacity exceeded; stream continuity lost")
            }
            Self::Closed => f.write_str("Live session is closing or closed"),
        }
    }
}

impl std::error::Error for Error {}

impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// An incomplete diagnostic HTTP body; status and headers remain authoritative.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HttpBodyIssue {
    Truncated,
    ReadFailed,
    /// A rejected 101 upgrade did not establish a valid WebSocket or a proven
    /// complete HTTP entity. No second request is made to guess its body.
    Unconfirmed,
}
