use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};
use std::fmt;

/// Released public Live voice model.
pub const GPT_LIVE_1: &str = "gpt-live-1";

/// A nullable optional field. Unlike `Option`, omission and explicit JSON null
/// remain distinct, including in sparse updates.
#[derive(Clone, Default, PartialEq, Eq)]
pub enum Field<T> {
    #[default]
    Absent,
    Null,
    Value(T),
}

impl<T> fmt::Debug for Field<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Absent => "Absent",
            Self::Null => "Null",
            Self::Value(_) => "Value([redacted])",
        })
    }
}

impl<T> Field<T> {
    #[must_use]
    pub const fn is_absent(&self) -> bool {
        matches!(self, Self::Absent)
    }

    #[must_use]
    pub const fn value(&self) -> Option<&T> {
        match self {
            Self::Value(value) => Some(value),
            _ => None,
        }
    }
}

impl<T: Serialize> Serialize for Field<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Value(value) => value.serialize(serializer),
            Self::Null => serializer.serialize_none(),
            Self::Absent => Err(serde::ser::Error::custom("absent field must be omitted")),
        }
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Field<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Option::<T>::deserialize(deserializer)?.map_or(Self::Null, Self::Value))
    }
}

/// Deserialize an optional but non-nullable field. Omission uses serde's default.
pub(super) fn nonnull<'de, T: Deserialize<'de>, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    T::deserialize(deserializer).map(Some)
}

/// Required, explicitly nullable value. Its containing field must not use a default.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Nullable<T>(pub Option<T>);

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionConfig {
    pub model: String,
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

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            model: GPT_LIVE_1.into(),
            audio: None,
            client: None,
            delegation: Field::Absent,
            input: None,
            instructions: Field::Absent,
            store: None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioConfig {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "nonnull"
    )]
    pub format: Option<AudioFormat>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "nonnull"
    )]
    pub output: Option<AudioOutput>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AudioFormat {
    #[serde(rename = "audio/pcm")]
    Pcm { rate: u32 },
    #[serde(rename = "audio/pcmu")]
    Pcmu { rate: u32 },
    #[serde(rename = "audio/pcma")]
    Pcma { rate: u32 },
}

impl Default for AudioFormat {
    fn default() -> Self {
        Self::Pcm { rate: 24_000 }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioOutput {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "nonnull"
    )]
    pub voice: Option<Voice>,
}

/// Public voice names are extensible; custom voices require provider authorization.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Voice {
    Named(String),
    Custom { id: String },
}

/// The names enumerated by the released Live schema (the default is `marin`).
pub const VOICES: &[&str] = &[
    "alloy", "ash", "ballad", "beacon", "bossa", "cedar", "cinder", "coral", "delta", "echo",
    "gleam", "marin", "meridian", "quartz", "ripple", "sage", "shimmer", "stone", "tempo", "verse",
    "vesper", "willow",
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientConfig {
    pub data_channel: DataChannelConfig,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataChannelConfig {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "nonnull"
    )]
    pub allowed_client_events: Option<EventPermissions<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "nonnull"
    )]
    pub allowed_server_events: Option<EventPermissions<ServerEventSelector>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EventPermissions<T> {
    All(AllEvents),
    Selected(Vec<T>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AllEvents {
    #[serde(rename = "all")]
    All,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerEventSelector {
    #[serde(rename = "type")]
    pub event_type: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "nonnull"
    )]
    pub response_event: Option<String>,
}

/// Startup history is text-only and has no `system` role.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InitialItem {
    pub role: InitialRole,
    pub content: Vec<InitialText>,
    #[serde(default, skip_serializing_if = "Field::is_absent")]
    pub id: Field<String>,
    #[serde(default, skip_serializing_if = "Field::is_absent")]
    pub status: Field<InitialStatus>,
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "nonnull"
    )]
    pub item_type: Option<MessageType>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InitialRole {
    Developer,
    User,
    Assistant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InitialStatus {
    Incomplete,
    Completed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageType {
    #[serde(rename = "message")]
    Message,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InitialText {
    pub text: String,
    #[serde(
        rename = "type",
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "nonnull"
    )]
    pub text_type: Option<InitialTextType>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InitialTextType {
    InputText,
    Text,
    OutputText,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DelegationConfig {
    Client,
    Responses { responses: ResponsesConfig },
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResponsesConfig {
    pub model: String,
    #[serde(flatten)]
    pub options: ResponsesOptions,
}

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResponsesOptions {
    #[serde(default, skip_serializing_if = "Field::is_absent")]
    pub instructions: Field<String>,
    #[serde(default, skip_serializing_if = "Field::is_absent")]
    pub max_output_tokens: Field<u64>,
    #[serde(default, skip_serializing_if = "Field::is_absent")]
    pub parallel_tool_calls: Field<bool>,
    #[serde(default, skip_serializing_if = "Field::is_absent")]
    pub reasoning: Field<Reasoning>,
    #[serde(default, skip_serializing_if = "Field::is_absent")]
    pub service_tier: Field<ServiceTier>,
    #[serde(default, skip_serializing_if = "Field::is_absent")]
    pub text: Field<TextConfig>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "nonnull"
    )]
    pub tool_choice: Option<ToolChoice>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "nonnull"
    )]
    pub tools: Option<Vec<Tool>>,
}

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionUpdate {
    #[serde(default, skip_serializing_if = "Field::is_absent")]
    pub delegation: Field<DelegationUpdate>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DelegationUpdate {
    Client,
    Responses {
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "nonnull"
        )]
        responses: Option<ResponsesUpdate>,
    },
}

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResponsesUpdate {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "nonnull"
    )]
    pub model: Option<String>,
    #[serde(flatten)]
    pub options: ResponsesOptions,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reasoning {
    #[serde(default, skip_serializing_if = "Field::is_absent")]
    pub effort: Field<ReasoningEffort>,
    #[serde(default, skip_serializing_if = "Field::is_absent")]
    pub summary: Field<ReasoningSummary>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningEffort {
    None,
    Minimal,
    Low,
    Medium,
    High,
    Xhigh,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningSummary {
    Concise,
    Detailed,
    Auto,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceTier {
    Auto,
    Default,
    FastTierTempPilot,
    Flex,
    Priority,
    Ultrafast,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextConfig {
    #[serde(default, skip_serializing_if = "Field::is_absent")]
    pub verbosity: Field<Verbosity>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verbosity {
    Low,
    Medium,
    High,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolChoice {
    Mode(ToolChoiceMode),
    Named(NamedToolChoice),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolChoiceMode {
    Auto,
    None,
    Required,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NamedToolChoice {
    Function {
        name: String,
    },
    /// A schema-supported selector, not permission to register arbitrary MCP tools.
    Mcp {
        name: String,
        server_label: String,
    },
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Tool {
    Function {
        name: String,
        #[serde(default, skip_serializing_if = "Field::is_absent")]
        description: Field<String>,
        #[serde(default, skip_serializing_if = "Field::is_absent")]
        parameters: Field<Map<String, Value>>,
        #[serde(default, skip_serializing_if = "Field::is_absent")]
        strict: Field<bool>,
    },
    WebSearch,
}

macro_rules! redacted_debug {
    ($($name:ty),+ $(,)?) => {$(
        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(concat!(stringify!($name)," { [redacted] }"))
            }
        }
    )+};
}

redacted_debug!(
    SessionConfig,
    InitialItem,
    InitialText,
    DelegationConfig,
    ResponsesConfig,
    ResponsesOptions,
    SessionUpdate,
    DelegationUpdate,
    ResponsesUpdate,
    Tool
);
