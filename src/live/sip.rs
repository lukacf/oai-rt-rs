//! SIP call control. Verify webhook signatures and deduplicate decisions in the
//! application before using these types; deserialization does not authenticate a call.
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::fmt;

use super::{Error, LiveClient, Result, SessionConfig};

/// SIP acceptance is intentionally distinct from primary/RTC startup: the
/// released SIP guide includes `type: "live"` here, unlike those transports.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SipAcceptSession {
    #[serde(rename = "type")]
    pub session_type: SipSessionType,
    #[serde(flatten)]
    pub config: SessionConfig,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SipSessionType {
    #[serde(rename = "live")]
    Live,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcceptRequest {
    pub session: SipAcceptSession,
}

impl SipAcceptSession {
    #[must_use]
    pub const fn new(config: SessionConfig) -> Self {
        Self {
            session_type: SipSessionType::Live,
            config,
        }
    }
}

impl AcceptRequest {
    /// # Errors
    /// Rejects invalid config and fields belonging to other primary transports.
    pub fn validate(&self) -> Result<()> {
        self.session.config.validate()?;
        if self
            .session
            .config
            .audio
            .as_ref()
            .and_then(|audio| audio.format)
            .is_some()
        {
            return Err(Error::Invalid(
                "SIP negotiates media; omit audio.format".into(),
            ));
        }
        if self.session.config.client.is_some() {
            return Err(Error::Invalid(
                "frontend data-channel permissions belong to WebRTC, not SIP".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RejectRequest {
    pub status_code: u16,
}

impl RejectRequest {
    /// # Errors
    /// Rejects statuses outside the documented inclusive 300-699 range.
    pub fn validate(self) -> Result<()> {
        if !(300..=699).contains(&self.status_code) {
            return Err(Error::Invalid(
                "SIP rejection status must be 300 through 699".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReferRequest {
    pub target_uri: String,
}

impl ReferRequest {
    /// # Errors
    /// Rejects a blank destination. The provider validates URI interpretation.
    pub fn validate(&self) -> Result<()> {
        if self.target_uri.trim().is_empty() {
            return Err(Error::Invalid("transfer target must not be blank".into()));
        }
        Ok(())
    }
}

macro_rules! redacted {
    ($($name:ty),+ $(,)?) => {$(
        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(concat!(stringify!($name)," { [redacted] }"))
            }
        }
    )+};
}
redacted!(SipAcceptSession, AcceptRequest, ReferRequest);

impl LiveClient {
    /// Accept an inbound SIP session. The first accept/reject decision wins.
    /// Keep a sideband for lifecycle and final usage; this does not attach one.
    ///
    /// # Errors
    /// Returns validation, HTTP errors, or ambiguous delivery. Never retries.
    pub async fn accept_call(&self, session_id: &str, request: &AcceptRequest) -> Result<()> {
        request.validate()?;
        let response = self
            .http
            .post(self.endpoint(&["live", "sessions", session_id, "accept"])?)
            .json(request)
            .send()
            .await
            .map_err(|_| Error::AmbiguousWrite)?;
        self.empty_response(response).await
    }

    /// Reject an inbound SIP session. Delivery acknowledgment is not an accept.
    ///
    /// # Errors
    /// Returns invalid status, HTTP errors, or ambiguous delivery. Never retries.
    pub async fn reject_call(&self, session_id: &str, request: RejectRequest) -> Result<()> {
        request.validate()?;
        let response = self
            .http
            .post(self.endpoint(&["live", "sessions", session_id, "reject"])?)
            .json(&request)
            .send()
            .await
            .map_err(|_| Error::AmbiguousWrite)?;
        self.empty_response(response).await
    }

    /// Request transfer of a call. The provider owns downstream transfer completion.
    ///
    /// # Errors
    /// Returns invalid URI, HTTP errors, or ambiguous delivery. Never retries.
    pub async fn refer_call(&self, session_id: &str, request: &ReferRequest) -> Result<()> {
        request.validate()?;
        let response = self
            .http
            .post(self.endpoint(&["live", "sessions", session_id, "refer"])?)
            .json(request)
            .send()
            .await
            .map_err(|_| Error::AmbiguousWrite)?;
        self.empty_response(response).await
    }

    /// Request session hangup with no HTTP body. A successful request is not final
    /// usage: continue observing the sideband until `session.closed`.
    ///
    /// # Errors
    /// Returns HTTP errors or ambiguous delivery. Never retries.
    pub async fn hangup(&self, session_id: &str) -> Result<()> {
        let response = self
            .http
            .post(self.endpoint(&["live", "sessions", session_id, "hangup"])?)
            .send()
            .await
            .map_err(|_| Error::AmbiguousWrite)?;
        self.empty_response(response).await
    }
}

/// Untrusted caller metadata. Never use a SIP header as authorization.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SipHeader {
    pub name: String,
    pub value: String,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IncomingSip {
    #[serde(rename = "type")]
    pub transport_type: SipTransportType,
    pub session_id: String,
    pub sip_headers: Vec<SipHeader>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacyIncomingSip {
    pub session_id: String,
    pub sip_headers: Vec<SipHeader>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SipTransportType {
    #[serde(rename = "sip")]
    Sip,
}

#[derive(Clone, PartialEq, Deserialize)]
#[serde(tag = "type")]
pub enum IncomingWebhookEvent {
    #[serde(rename = "live.transport.incoming")]
    Incoming {
        id: String,
        created_at: f64,
        data: IncomingSip,
        #[serde(default, deserialize_with = "super::models::nonnull")]
        object: Option<WebhookObject>,
    },
    #[serde(rename = "live.call.incoming")]
    LegacyIncoming {
        id: String,
        created_at: f64,
        data: LegacyIncomingSip,
        #[serde(default, deserialize_with = "super::models::nonnull")]
        object: Option<WebhookObject>,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WebhookObject {
    #[serde(rename = "event")]
    Event,
}

/// Parsed webhook plus its lossless body. This is not signature verification.
#[derive(Clone, PartialEq)]
pub struct IncomingWebhook {
    pub event: IncomingWebhookEvent,
    pub raw: Value,
}

impl<'de> Deserialize<'de> for IncomingWebhook {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let raw = Value::deserialize(deserializer)?;
        let event = serde_json::from_value(raw.clone()).map_err(serde::de::Error::custom)?;
        Ok(Self { event, raw })
    }
}

redacted!(
    SipHeader,
    IncomingSip,
    LegacyIncomingSip,
    IncomingWebhookEvent,
    IncomingWebhook
);
