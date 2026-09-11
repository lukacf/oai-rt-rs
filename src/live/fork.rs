use serde::{Deserialize, Serialize};
use std::fmt;

use super::models::nonnull;
use super::{AudioFormat, ClientConfig, Error, Field, ResponsesUpdate, Result};

/// Stored-state overrides. A fork cannot replace model, original instructions,
/// input history, voice, or delegation mode.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForkSessionConfig {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "nonnull"
    )]
    pub audio: Option<ForkAudioConfig>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "nonnull"
    )]
    pub client: Option<ClientConfig>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "nonnull"
    )]
    pub delegation: Option<ForkDelegation>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "nonnull"
    )]
    pub store: Option<bool>,
}

impl fmt::Debug for ForkSessionConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ForkSessionConfig { [redacted] }")
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForkAudioConfig {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "nonnull"
    )]
    pub format: Option<AudioFormat>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ForkDelegation {
    #[serde(rename = "responses")]
    Responses {
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "nonnull"
        )]
        responses: Option<ResponsesUpdate>,
    },
}

/// The fork's first message has the same wire name but a different session schema
/// from new primary connections. Use only on `/sessions/{source}/fork`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForkStartEvent {
    #[serde(rename = "type")]
    pub event_type: ForkStartType,
    pub session: ForkSessionConfig,
    #[serde(default, skip_serializing_if = "Field::is_absent")]
    pub event_id: Field<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ForkStartType {
    #[serde(rename = "session.start")]
    Start,
}

impl ForkStartEvent {
    #[must_use]
    pub const fn new(session: ForkSessionConfig) -> Self {
        Self {
            event_type: ForkStartType::Start,
            session,
            event_id: Field::Absent,
        }
    }

    /// # Errors
    /// Rejects invalid fork overrides or oversized correlation IDs.
    pub fn validate(&self) -> Result<()> {
        self.session.validate(ForkTransport::WebSocket)?;
        if self
            .event_id
            .value()
            .is_some_and(|id| id.chars().count() > 512)
        {
            return Err(Error::Invalid("event_id exceeds 512 characters".into()));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForkTransport {
    WebSocket,
    WebRtc,
}

impl ForkSessionConfig {
    /// Validate overrides against the new fork's primary transport.
    ///
    /// # Errors
    /// Rejects frontend permissions on a WS fork and manual audio on WebRTC.
    pub fn validate(&self, transport: ForkTransport) -> Result<()> {
        if transport == ForkTransport::WebSocket && self.client.is_some() {
            return Err(Error::Invalid(
                "WebSocket forks discard frontend permissions; omit client".into(),
            ));
        }
        if transport == ForkTransport::WebRtc && self.audio.is_some() {
            return Err(Error::Invalid(
                "WebRTC forks negotiate audio; omit audio".into(),
            ));
        }
        if let Some(format) = self.audio.and_then(|audio| audio.format) {
            format.validate()?;
        }
        if let Some(client) = &self.client {
            client.validate()?;
        }
        if let Some(ForkDelegation::Responses {
            responses: Some(responses),
        }) = &self.delegation
        {
            responses.validate()?;
        }
        Ok(())
    }

    #[must_use]
    pub fn audio_format(&self) -> AudioFormat {
        self.audio
            .and_then(|audio| audio.format)
            .unwrap_or_default()
    }
}
