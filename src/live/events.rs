use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};
use std::fmt;

use super::models::nonnull;
use super::{
    AudioConfig, ClientConfig, DelegationConfig, Field, InitialItem, Nullable, SessionConfig,
    SessionUpdate,
};

/// A client command and its optional correlation identifier.
///
/// IDs correlate acknowledgments/rejections; they provide no idempotency guarantee.
#[derive(Clone, PartialEq, Serialize)]
pub struct ClientEvent {
    #[serde(default, skip_serializing_if = "Field::is_absent")]
    pub event_id: Field<String>,
    #[serde(flatten)]
    pub command: Command,
}

impl fmt::Debug for ClientEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientEvent")
            .field("type", &self.command.kind())
            .finish_non_exhaustive()
    }
}

impl ClientEvent {
    #[must_use]
    pub const fn new(command: Command) -> Self {
        Self {
            event_id: Field::Absent,
            command,
        }
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Command {
    #[serde(rename = "session.start")]
    Start { session: SessionConfig },
    #[serde(rename = "session.update")]
    Update { session: SessionUpdate },
    #[serde(rename = "session.close")]
    Close,
    #[serde(rename = "session.input_audio.append")]
    InputAudioAppend { audio: String },
    #[serde(rename = "session.input_audio.mute")]
    InputAudioMute,
    #[serde(rename = "session.input_audio.unmute")]
    InputAudioUnmute,
    #[serde(rename = "session.instructions.append")]
    InstructionsAppend {
        content: String,
        delegation_id: Nullable<String>,
    },
    #[serde(rename = "session.thinking.append")]
    ThinkingAppend {
        content: String,
        delegation_id: Nullable<String>,
    },
    #[serde(rename = "session.commentary.append")]
    CommentaryAppend {
        content: String,
        delegation_id: Nullable<String>,
    },
    #[serde(rename = "response.item.create")]
    ResponseItemCreate { item: super::ResponseInputItem },
    #[serde(rename = "response.create")]
    ResponseCreate,
}

impl Command {
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Start { .. } => "session.start",
            Self::Update { .. } => "session.update",
            Self::Close => "session.close",
            Self::InputAudioAppend { .. } => "session.input_audio.append",
            Self::InputAudioMute => "session.input_audio.mute",
            Self::InputAudioUnmute => "session.input_audio.unmute",
            Self::InstructionsAppend { .. } => "session.instructions.append",
            Self::ThinkingAppend { .. } => "session.thinking.append",
            Self::CommentaryAppend { .. } => "session.commentary.append",
            Self::ResponseItemCreate { .. } => "response.item.create",
            Self::ResponseCreate => "response.create",
        }
    }
}

/// Snapshot status stays active, even inside the terminal `session.closed` event.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionSnapshot {
    pub id: String,
    /// UNIX seconds, not session-relative milliseconds.
    pub expires_at: f64,
    pub model: String,
    pub status: SessionStatus,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "nonnull"
    )]
    pub audio: Option<AudioConfig>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "nonnull"
    )]
    pub client: Option<ClientConfig>,
    #[serde(default, skip_serializing_if = "Field::is_absent")]
    pub delegation: Field<DelegationConfig>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "nonnull"
    )]
    pub input: Option<Vec<InitialItem>>,
    #[serde(default, skip_serializing_if = "Field::is_absent")]
    pub instructions: Field<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "nonnull"
    )]
    pub store: Option<bool>,
}

impl fmt::Debug for SessionSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SessionSnapshot")
            .field("status", &self.status)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Active,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderError {
    /// The lifecycle guide permits null even though the shared schema says string.
    #[serde(deserialize_with = "nullable_error_code")]
    pub code: Option<String>,
    pub message: String,
    #[serde(rename = "type")]
    pub error_type: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "nonnull"
    )]
    pub client_event_id: Option<String>,
    /// Public provider errors also emit explicit null for an unscoped parameter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub param: Option<String>,
}

fn nullable_error_code<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    Option::<String>::deserialize(deserializer)
}

impl fmt::Debug for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ProviderError { [redacted] }")
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallError {
    pub code: String,
    pub message: String,
    #[serde(rename = "type")]
    pub error_type: CallErrorType,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "nonnull"
    )]
    pub param: Option<String>,
}

impl fmt::Debug for CallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CallError { [redacted] }")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CallErrorType {
    #[serde(rename = "call_error")]
    CallError,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    /// Cumulative session seconds. Replace previous snapshots; never sum them.
    pub seconds: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ContextWindow {
    pub usage_ratio: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloseReason {
    CloseRequested,
    Expired,
    Content,
    RemoteHangup,
    ConnectionLost,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Delegation {
    pub id: String,
    #[serde(rename = "type")]
    pub item_type: DelegationType,
    pub target: DelegationTarget,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "nonnull"
    )]
    pub response_id: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DelegationType {
    #[serde(rename = "delegation")]
    Delegation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DelegationTarget {
    Client,
    Responses,
}

/// Recognized server events. Unknown fields and events are retained by
/// [`ServerFrame`] rather than being silently discarded.
#[derive(Clone, PartialEq, Deserialize)]
#[serde(tag = "type")]
pub enum ServerEvent {
    #[serde(rename = "session.started")]
    Started {
        event_id: String,
        session: SessionSnapshot,
    },
    #[serde(rename = "session.updated")]
    Updated {
        event_id: String,
        session: SessionSnapshot,
    },
    #[serde(rename = "session.closed")]
    Closed {
        event_id: String,
        session: SessionSnapshot,
        reason: CloseReason,
        usage: Usage,
    },
    #[serde(rename = "session.input_audio.muted")]
    InputAudioMuted { event_id: String },
    #[serde(rename = "session.input_audio.unmuted")]
    InputAudioUnmuted { event_id: String },
    #[serde(rename = "session.instructions.appended")]
    InstructionsAppended {
        event_id: String,
        start_ms: f64,
        end_ms: f64,
    },
    #[serde(rename = "session.thinking.appended")]
    ThinkingAppended {
        event_id: String,
        start_ms: f64,
        end_ms: f64,
    },
    #[serde(rename = "session.commentary.appended")]
    CommentaryAppended {
        event_id: String,
        start_ms: f64,
        end_ms: f64,
    },
    #[serde(rename = "session.output_audio.delta")]
    OutputAudioDelta {
        delta: String,
        #[serde(default, deserialize_with = "nonnull")]
        start_ms: Option<f64>,
        #[serde(default, deserialize_with = "nonnull")]
        end_ms: Option<f64>,
    },
    /// Reflected sideband input, before model-input muting. Not an acknowledgment.
    #[serde(rename = "session.input_audio.append")]
    InputAudio { audio: String },
    #[serde(rename = "session.input_transcript.delta")]
    InputTranscriptDelta {
        event_id: String,
        delta: String,
        start_ms: f64,
        end_ms: f64,
    },
    #[serde(rename = "session.output_transcript.delta")]
    OutputTranscriptDelta {
        event_id: String,
        delta: String,
        start_ms: f64,
        end_ms: f64,
    },
    #[serde(rename = "session.delegation.created")]
    DelegationCreated {
        event_id: String,
        offset_ms: f64,
        delegation: Delegation,
    },
    #[serde(rename = "response.event")]
    Response {
        event_id: String,
        #[serde(default)]
        delegation_id: Field<String>,
        event: Map<String, Value>,
    },
    #[serde(rename = "session.usage.updated")]
    UsageUpdated {
        event_id: String,
        usage: Usage,
        #[serde(default, deserialize_with = "nonnull")]
        context_window: Option<ContextWindow>,
    },
    #[serde(rename = "error")]
    Error {
        event_id: String,
        error: ProviderError,
    },
    #[serde(rename = "info")]
    Info {
        event_id: String,
        code: String,
        message: String,
    },
    #[serde(rename = "transport.dtmf.received")]
    DtmfReceived {
        event_id: String,
        #[serde(deserialize_with = "dtmf_event")]
        event: String,
    },
    #[serde(rename = "transport.dtmf.send")]
    DtmfSend {
        event_id: String,
        #[serde(deserialize_with = "dtmf_event")]
        event: String,
    },
    #[serde(rename = "transport.ringing")]
    Ringing {
        event_id: String,
        session_id: String,
    },
    #[serde(rename = "transport.answered")]
    Answered {
        event_id: String,
        session_id: String,
    },
    #[serde(rename = "transport.failed")]
    TransportFailed {
        event_id: String,
        session_id: String,
        error: CallError,
    },
    #[serde(other)]
    Unknown,
}

fn dtmf_event<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let event = String::deserialize(deserializer)?;
    if event.len() != 1 || !b"0123456789*#ABCD".contains(&event.as_bytes()[0]) {
        return Err(serde::de::Error::custom(
            "DTMF event must be one of 0-9, *, #, A-D",
        ));
    }
    Ok(event)
}

impl fmt::Debug for ServerEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ServerEvent { [payload redacted] }")
    }
}

/// A lossless inbound event with a typed view. Access `raw` explicitly when
/// examining extensions; Debug never prints payloads or personal data.
#[derive(Clone, PartialEq)]
pub struct ServerFrame {
    pub event: ServerEvent,
    pub client_event_id: Option<String>,
    pub raw: Value,
}

impl ServerFrame {
    /// Decode a wrapped Responses event while keeping its outer Live correlation
    /// and delegation metadata on this frame.
    ///
    /// # Errors
    /// Rejects malformed known nested events instead of exposing actionable calls.
    pub fn response_event(&self) -> super::Result<Option<super::ResponseEvent>> {
        match &self.event {
            ServerEvent::Response { event, .. } => super::ResponseEvent::decode(Value::Object(event.clone()))
                .map(Some)
                .map_err(|source| super::Error::MalformedEvent {raw:self.raw.clone(),source}),
            _ => Ok(None),
        }
    }
}

impl fmt::Debug for ServerFrame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ServerFrame { [payload redacted] }")
    }
}

impl<'de> Deserialize<'de> for ServerFrame {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = Value::deserialize(deserializer)?;
        let event = serde_json::from_value(raw.clone()).map_err(serde::de::Error::custom)?;
        let client_event_id = raw
            .get("client_event_id")
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| serde::de::Error::custom("client_event_id must be a string"))
            })
            .transpose()?;
        Ok(Self {
            event,
            client_event_id,
            raw,
        })
    }
}
