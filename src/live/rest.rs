use reqwest::{Response, header::CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use std::fmt;

use super::models::nonnull;
use super::{
    Error, ForkSessionConfig, ForkTransport, HttpBodyIssue, LiveClient, Result, SessionConfig,
};

/// Public WebRTC bootstrap JSON. Model/configuration belong in `session`.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateRequest {
    pub session: SessionConfig,
    pub transport: WebRtcTransport,
}

/// SDP signaling only; capture, ICE/DTLS, media tracks and playback belong to the peer.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WebRtcTransport {
    #[serde(rename = "webrtc")]
    WebRtc { sdp: String },
}

impl WebRtcTransport {
    #[must_use]
    pub fn sdp(&self) -> &str {
        match self {
            Self::WebRtc { sdp } => sdp,
        }
    }
}

/// Creation responses identify the session but are not `session.started` snapshots.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreatedSession {
    pub id: String,
}

/// HTTP 201 public signaling result, including the answer SDP.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateResponse {
    pub session: CreatedSession,
    pub transport: WebRtcTransport,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForkRequest {
    pub transport: WebRtcTransport,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "nonnull"
    )]
    pub session: Option<ForkSessionConfig>,
}

macro_rules! redacted_debug {
    ($($name:ty),+ $(,)?) => {$(
        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(concat!(stringify!($name), " { [redacted] }"))
            }
        }
    )+};
}
redacted_debug!(CreateRequest, CreateResponse, WebRtcTransport, ForkRequest);

impl CreateRequest {
    /// # Errors
    /// Rejects invalid startup configuration, empty SDP, and media format overrides.
    pub fn validate(&self) -> Result<()> {
        self.session.validate()?;
        if self
            .session
            .audio
            .as_ref()
            .and_then(|audio| audio.format)
            .is_some()
        {
            return Err(Error::Invalid(
                "WebRTC negotiates media; omit audio.format".into(),
            ));
        }
        if self.transport.sdp().is_empty() {
            return Err(Error::Invalid("SDP offer must not be empty".into()));
        }
        Ok(())
    }
}

/// Streaming stereo WAV response. No recording is written to disk automatically.
pub struct ContentDownload {
    response: Response,
    pub metadata: ContentMetadata,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentMetadata {
    pub content_type: String,
    pub content_length: Option<u64>,
    pub request_id: Option<String>,
}

impl fmt::Debug for ContentDownload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ContentDownload { [recording redacted] }")
    }
}

impl ContentDownload {
    /// Receive the next content chunk without buffering the whole recording.
    ///
    /// # Errors
    /// Returns an error for an interrupted or timed-out download.
    pub async fn next_chunk(&mut self) -> Result<Option<Vec<u8>>> {
        self.response
            .chunk()
            .await
            .map(|chunk| chunk.map(|bytes| bytes.to_vec()))
            .map_err(|_| Error::Transport("recording download".into()))
    }

    /// Collect a recording subject to an explicit caller-owned byte bound.
    ///
    /// # Errors
    /// Rejects oversized content or a failed stream without returning partial success.
    pub async fn read_all(mut self, max_bytes: usize) -> Result<Vec<u8>> {
        if self
            .metadata
            .content_length
            .is_some_and(|size| size > max_bytes as u64)
        {
            return Err(Error::Invalid(
                "recording exceeds configured byte limit".into(),
            ));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = self.next_chunk().await? {
            if chunk.len() > max_bytes.saturating_sub(bytes.len()) {
                return Err(Error::Invalid(
                    "recording exceeds configured byte limit".into(),
                ));
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }
}

impl LiveClient {
    /// Create a new WebRTC session from completed stored state.
    ///
    /// # Errors
    /// Returns validation, provider, malformed response, or ambiguous-write errors.
    pub async fn fork_webrtc(
        &self,
        source_session_id: &str,
        request: &ForkRequest,
    ) -> Result<CreateResponse> {
        if let Some(session) = &request.session {
            session.validate(ForkTransport::WebRtc)?;
        }
        if request.transport.sdp().is_empty() {
            return Err(Error::Invalid("SDP offer must not be empty".into()));
        }
        let response = self
            .http
            .post(self.endpoint(&["live", "sessions", source_session_id, "fork"])?)
            .json(request)
            .send()
            .await
            .map_err(|_| Error::AmbiguousWrite)?;
        let created: CreateResponse = self.json_response(response, 201).await?;
        if created.session.id == source_session_id {
            return Err(Error::Invalid(
                "fork must return a new session identifier".into(),
            ));
        }
        Ok(created)
    }

    /// Create a public WebRTC session and return its answer. Do not send
    /// `session.start` again on the data channel. Attach a sideband before applying
    /// the answer if the application needs to observe initial events.
    ///
    /// # Errors
    /// Returns validation, HTTP/schema, or ambiguous-delivery failures. Never retries.
    pub async fn create_webrtc(&self, request: &CreateRequest) -> Result<CreateResponse> {
        request.validate()?;
        let response = self
            .http
            .post(self.endpoint(&["live", "sessions"])?)
            .json(request)
            .send()
            .await
            .map_err(|_| Error::AmbiguousWrite)?;
        self.json_response(response, 201).await
    }

    /// Stream stored session content (stereo WAV, input left/output right).
    /// Storage must have been enabled at session creation; no project settings
    /// or retention policy are modified.
    ///
    /// # Errors
    /// Returns HTTP failures, invalid content types, or network errors.
    pub async fn download_content(&self, session_id: &str) -> Result<ContentDownload> {
        if !session_id.strip_prefix("live_").is_some_and(|suffix| {
            (1..=128).contains(&suffix.len())
                && suffix
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        }) {
            return Err(Error::Invalid(
                "recording ID must match live_ followed by 1-128 URL-safe characters".into(),
            ));
        }
        let response = self
            .http
            .get(self.endpoint(&["live", "sessions", session_id, "content"])?)
            .send()
            .await
            .map_err(|_| Error::Transport("recording request".into()))?;
        if response.status().as_u16() != 200 {
            return Err(self.http_error(response).await);
        }
        let content_type = header(&response, "content-type")
            .ok_or_else(|| Error::Invalid("recording response lacks content type".into()))?;
        if !media_type(&content_type).eq_ignore_ascii_case("audio/wav") {
            return Err(Error::Invalid(
                "recording response must be audio/wav".into(),
            ));
        }
        let metadata = ContentMetadata {
            content_type,
            content_length: response.content_length(),
            request_id: header(&response, "x-request-id"),
        };
        Ok(ContentDownload { response, metadata })
    }

    async fn json_response<T: serde::de::DeserializeOwned>(
        &self,
        response: Response,
        status: u16,
    ) -> Result<T> {
        if response.status().as_u16() != status {
            return Err(self.http_error(response).await);
        }
        if response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_none_or(|value| !media_type(value).eq_ignore_ascii_case("application/json"))
        {
            return Err(Error::Invalid("expected application/json response".into()));
        }
        let bytes = bounded_body(response, self.options.codec.max_event_bytes).await?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    pub(super) async fn empty_response(&self, response: Response) -> Result<()> {
        if response.status().as_u16() != 200 {
            return Err(self.http_error(response).await);
        }
        let body = bounded_body(response, self.options.codec.max_event_bytes).await?;
        if !body.is_empty() {
            return Err(Error::Invalid(
                "call control success must have an empty body".into(),
            ));
        }
        Ok(())
    }

    async fn http_error(&self, response: Response) -> Error {
        self.http_error_until(
            response,
            tokio::time::Instant::now() + self.options.request_timeout,
        )
        .await
    }

    pub(super) async fn http_error_until(
        &self,
        response: Response,
        deadline: tokio::time::Instant,
    ) -> Error {
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let request_id = header(&response, "x-request-id");
        let retry_after = header(&response, "retry-after");
        let content_type = header(&response, "content-type");
        let (body, body_issue) =
            diagnostic_body(response, self.options.codec.max_event_bytes, deadline).await;
        Error::Http {
            status,
            headers: Box::new(headers),
            request_id,
            body,
            content_type,
            retry_after,
            body_issue,
        }
    }
}

fn header(response: &Response, name: &str) -> Option<String> {
    response
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

fn media_type(value: &str) -> &str {
    value.split(';').next().unwrap_or("").trim()
}

async fn diagnostic_body(
    mut response: Response,
    limit: usize,
    deadline: tokio::time::Instant,
) -> (Vec<u8>, Option<HttpBodyIssue>) {
    let declared_oversize = response
        .content_length()
        .is_some_and(|size| size > limit as u64);
    let mut body = Vec::new();
    loop {
        match tokio::time::timeout_at(deadline, response.chunk()).await {
            Ok(Ok(Some(chunk))) => {
                let remaining = limit.saturating_sub(body.len());
                let kept = chunk.len().min(remaining);
                body.extend_from_slice(&chunk[..kept]);
                if kept < chunk.len() || body.len() == limit && declared_oversize {
                    return (body, Some(HttpBodyIssue::Truncated));
                }
            }
            Ok(Ok(None)) => return (body, None),
            Ok(Err(_)) | Err(_) => return (body, Some(HttpBodyIssue::ReadFailed)),
        }
    }
}

async fn bounded_body(mut response: Response, limit: usize) -> Result<Vec<u8>> {
    if response
        .content_length()
        .is_some_and(|size| size > limit as u64)
    {
        return Err(Error::Invalid(
            "HTTP body exceeds configured byte limit".into(),
        ));
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| Error::Transport("HTTP response body".into()))?
    {
        if chunk.len() > limit.saturating_sub(body.len()) {
            return Err(Error::Invalid(
                "HTTP body exceeds configured byte limit".into(),
            ));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}
