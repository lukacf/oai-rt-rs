use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use std::{fmt, time::Duration};
use url::Url;

use super::{Codec, Error, Result};

/// Transport bounds and standard project/organization authentication.
#[derive(Clone)]
pub struct ClientOptions {
    pub organization: Option<String>,
    pub project: Option<String>,
    /// API root, including `/v1/`. Plain HTTP is allowed only on loopback.
    pub base_url: Url,
    pub request_timeout: Duration,
    pub codec: Codec,
    pub command_capacity: usize,
    pub event_capacity: usize,
}

impl fmt::Debug for ClientOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientOptions")
            .field("request_timeout", &self.request_timeout)
            .field("codec", &self.codec)
            .field("command_capacity", &self.command_capacity)
            .field("event_capacity", &self.event_capacity)
            .finish_non_exhaustive()
    }
}

impl Default for ClientOptions {
    fn default() -> Self {
        Self {
            organization: None,
            project: None,
            base_url: Url::parse("https://api.openai.com/v1/").expect("constant API URL"),
            request_timeout: Duration::from_secs(30),
            codec: Codec::default(),
            command_capacity: 16,
            event_capacity: 16,
        }
    }
}

/// Authenticated public Live HTTP and WebSocket client.
///
/// Credentials stay in sensitive headers. No Alpha/Beta header is added, redirects
/// are disabled, and failed or ambiguous writes are never automatically retried.
#[derive(Clone)]
pub struct LiveClient {
    pub(super) http: reqwest::Client,
    pub(super) websocket_http: reqwest::Client,
    pub(super) options: ClientOptions,
}

impl fmt::Debug for LiveClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("LiveClient { [credentials redacted] }")
    }
}

impl LiveClient {
    /// # Errors
    /// Returns an error for invalid credentials or HTTP client initialization.
    pub fn new(api_key: &str) -> Result<Self> {
        Self::with_options(api_key, ClientOptions::default())
    }

    /// # Errors
    /// Rejects invalid headers, unsafe API roots, and zero queue/timeout bounds.
    pub fn with_options(api_key: &str, options: ClientOptions) -> Result<Self> {
        let url = &options.base_url;
        let loopback = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
        if !(url.scheme() == "https" || url.scheme() == "http" && loopback)
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || !url.path().ends_with('/')
        {
            return Err(Error::Invalid(
                "API root must be HTTPS, credential-free, and end in /".into(),
            ));
        }
        if api_key.is_empty()
            || options.command_capacity == 0
            || options.event_capacity == 0
            || options.codec.max_event_bytes == 0
            || options.request_timeout.is_zero()
        {
            return Err(Error::Invalid(
                "credentials and transport bounds must be nonempty".into(),
            ));
        }
        let mut headers = HeaderMap::new();
        let mut auth = HeaderValue::from_str(&format!("Bearer {api_key}"))
            .map_err(|_| Error::Invalid("invalid authorization header".into()))?;
        auth.set_sensitive(true);
        headers.insert(AUTHORIZATION, auth);
        for (name, value) in [
            ("OpenAI-Organization", options.organization.as_deref()),
            ("OpenAI-Project", options.project.as_deref()),
        ] {
            if let Some(value) = value {
                let mut header = HeaderValue::from_str(value)
                    .map_err(|_| Error::Invalid("invalid organization/project header".into()))?;
                header.set_sensitive(true);
                headers.insert(name, header);
            }
        }
        let http = reqwest::Client::builder()
            .default_headers(headers.clone())
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .timeout(options.request_timeout)
            .build()
            .map_err(|_| Error::Transport("HTTP client initialization".into()))?;
        let websocket_http = reqwest::Client::builder()
            .default_headers(headers)
            .http1_only()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .timeout(options.request_timeout)
            .build()
            .map_err(|_| Error::Transport("WebSocket HTTP client initialization".into()))?;
        Ok(Self {
            http,
            websocket_http,
            options,
        })
    }

    pub(super) fn endpoint(&self, segments: &[&str]) -> Result<Url> {
        if segments
            .iter()
            .any(|s| s.is_empty() || *s == "." || *s == "..")
        {
            return Err(Error::Invalid(
                "empty or relative endpoint identifier".into(),
            ));
        }
        let mut url = self.options.base_url.clone();
        url.path_segments_mut()
            .map_err(|()| Error::Invalid("invalid API root".into()))?
            .pop_if_empty()
            .extend(segments);
        Ok(url)
    }
}
