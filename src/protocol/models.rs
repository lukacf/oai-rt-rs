use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde::ser::SerializeStruct;
use serde_json::Value;
use std::collections::HashMap;

pub const DEFAULT_MODEL: &str = "gpt-realtime";

/// Arbitrary JSON payloads allowed by the API (e.g. metadata values).
pub type Metadata = HashMap<String, Value>;

/// JSON Schema / tool parameter definitions are intentionally untyped.
pub type JsonSchema = Value;

/// Free-form JSON payloads where the spec is open-ended.
pub type ArbitraryJson = Value;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    #[default]
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ItemStatus {
    #[default]
    InProgress,
    Completed,
    Incomplete,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Modality {
    #[default]
    Audio,
    Text,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputModalities {
    Audio,
    Text,
}

impl Serialize for OutputModalities {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let values = match self {
            Self::Audio => vec![Modality::Audio],
            Self::Text => vec![Modality::Text],
        };
        values.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for OutputModalities {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr {
            Single(Modality),
            Many(Vec<Modality>),
        }

        match Repr::deserialize(deserializer)? {
            Repr::Single(Modality::Audio) => Ok(Self::Audio),
            Repr::Single(Modality::Text) => Ok(Self::Text),
            Repr::Many(values) => match values.as_slice() {
                [Modality::Audio] => Ok(Self::Audio),
                [Modality::Text] => Ok(Self::Text),
                _ => Err(serde::de::Error::custom(
                    "output_modalities must contain exactly one of: audio or text",
                )),
            },
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ConversationMode {
    #[default]
    Auto,
    None,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SessionKind {
    #[default]
    Realtime,
    Transcription,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Eagerness {
    Auto,
    Low,
    #[default]
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum Voice {
    Id(String),
    Object { id: String },
}

impl<S: Into<String>> From<S> for Voice {
    fn from(s: S) -> Self {
        // Own the string to avoid lifetime plumbing in public APIs.
        Self::Id(s.into())
    }
}

impl std::fmt::Display for Voice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Id(id) | Self::Object { id } => write!(f, "{id}"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct BetaAudioFormat(pub String);

impl std::fmt::Display for BetaAudioFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AudioFormat {
    #[serde(rename = "audio/pcm")]
    Pcm { #[serde(default = "default_pcm_rate")] rate: u32 },
    #[serde(rename = "audio/pcmu")]
    Pcmu,
    #[serde(rename = "audio/pcma")]
    Pcma,
}

impl std::fmt::Display for AudioFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pcm { .. } => write!(f, "audio/pcm"),
            Self::Pcmu => write!(f, "audio/pcmu"),
            Self::Pcma => write!(f, "audio/pcma"),
        }
    }
}

const PCM_24KHZ_RATE: u32 = 24_000;

const fn default_pcm_rate() -> u32 {
    PCM_24KHZ_RATE
}

impl AudioFormat {
    #[must_use]
    pub const fn pcm_24khz() -> Self {
        Self::Pcm { rate: PCM_24KHZ_RATE }
    }

    /// # Errors
    /// Returns an error if a PCM format is configured with a non-24kHz rate.
    #[allow(clippy::result_large_err)]
    pub fn validate(&self) -> Result<(), crate::error::Error> {
        match self {
            Self::Pcm { rate } if *rate != PCM_24KHZ_RATE => Err(crate::error::Error::InvalidClientEvent(
                format!("audio/pcm rate must be {PCM_24KHZ_RATE}, got {rate}"),
            )),
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum MaxTokens {
    Count(u32),
    Infinite(Infinite),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Infinite {
    #[serde(rename = "inf")]
    Inf,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
#[serde(transparent)]
pub struct Temperature(f32);

impl Temperature {
    /// # Errors
    /// Returns an error if `val` is outside the inclusive range [0.0, 2.0].
    pub fn new(val: f32) -> Result<Self, TemperatureError> {
        if (0.0..=2.0).contains(&val) {
            Ok(Self(val))
        } else {
            Err(TemperatureError { value: val })
        }
    }
}

impl Default for Temperature {
    fn default() -> Self {
        Self(0.8)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TemperatureError {
    pub value: f32,
}

impl std::fmt::Display for TemperatureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "temperature must be between 0.0 and 2.0, got {}", self.value)
    }
}

impl std::error::Error for TemperatureError {}

impl TryFrom<f32> for Temperature {
    type Error = TemperatureError;

    fn try_from(value: f32) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl<'de> Deserialize<'de> for Temperature {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = f32::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum PromptRef {
    Id(String),
    Object { id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TracingAuto {
    Auto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TracingConfig {
    pub workflow_name: Option<String>,
    pub group_id: Option<String>,
    /// Arbitrary tracing metadata (spec allows free-form JSON values).
    pub metadata: Option<Metadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Tracing {
    Auto(TracingAuto),
    Config(TracingConfig),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TruncationStrategy {
    Auto,
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TruncationType {
    RetentionRatio,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TokenLimits {
    pub post_instructions: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RetentionRatioTruncation {
    #[serde(rename = "type")]
    pub kind: TruncationType,
    pub retention_ratio: f32,
    pub token_limits: Option<TokenLimits>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Truncation {
    Strategy(TruncationStrategy),
    RetentionRatio(RetentionRatioTruncation),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    #[serde(rename = "type")]
    pub kind: SessionKind,
    pub model: String,
    pub output_modalities: OutputModalities,
    pub modalities: Option<Vec<Modality>>,
    pub include: Option<Vec<String>>,
    pub prompt: Option<PromptRef>,
    pub truncation: Option<Truncation>,
    pub instructions: Option<String>,
    pub input_audio_format: Option<AudioFormat>,
    pub output_audio_format: Option<AudioFormat>,
    pub input_audio_transcription: Option<InputAudioTranscription>,
    pub turn_detection: Option<TurnDetection>,
    pub tools: Option<Vec<Tool>>,
    pub tool_choice: Option<ToolChoice>,
    pub temperature: Option<Temperature>,
    pub max_output_tokens: Option<MaxTokens>,
    pub audio: Option<AudioConfig>,
    pub tracing: Option<Tracing>,
    pub voice: Option<Voice>,
}

impl SessionConfig {
    #[must_use]
    pub fn new(
        kind: SessionKind,
        model: impl Into<String>,
        output_modalities: OutputModalities,
    ) -> Self {
        Self {
            kind,
            model: model.into(),
            output_modalities,
            modalities: None,
            include: None,
            prompt: None,
            truncation: None,
            instructions: None,
            input_audio_format: None,
            output_audio_format: None,
            input_audio_transcription: None,
            turn_detection: None,
            tools: None,
            tool_choice: None,
            temperature: None,
            max_output_tokens: None,
            audio: None,
            tracing: None,
            voice: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionUpdateConfig {
    /// Partial updates only; GA forbids changing `model`, `voice`, or session `type`.
    pub output_modalities: Option<OutputModalities>,
    pub modalities: Option<Vec<Modality>>,
    pub include: Option<Vec<String>>,
    pub prompt: Option<PromptRef>,
    pub truncation: Option<Truncation>,
    pub instructions: Option<String>,
    pub input_audio_format: Option<AudioFormat>,
    pub output_audio_format: Option<AudioFormat>,
    pub input_audio_transcription: Option<InputAudioTranscription>,
    pub turn_detection: Option<TurnDetection>,
    pub tools: Option<Vec<Tool>>,
    pub tool_choice: Option<ToolChoice>,
    pub temperature: Option<Temperature>,
    pub max_output_tokens: Option<MaxTokens>,
    pub audio: Option<AudioConfig>,
    pub tracing: Option<Tracing>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub object: String,
    pub expires_at: u64,
    /// Flattened to match the API's session JSON shape.
    #[serde(flatten)]
    pub config: SessionConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionUpdate {
    /// Flattened to match the API's session.update JSON shape.
    #[serde(flatten)]
    pub config: SessionUpdateConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AudioConfig {
    pub input: Option<InputAudioConfig>,
    pub output: Option<OutputAudioConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InputAudioConfig {
    pub format: Option<AudioFormat>,
    pub turn_detection: Option<TurnDetection>,
    pub transcription: Option<InputAudioTranscription>,
    pub noise_reduction: Option<NoiseReduction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NoiseReduction {
    #[serde(rename = "type")]
    pub kind: NoiseReductionType,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NoiseReductionType {
    Default,
}

impl Default for NoiseReductionType {
    fn default() -> Self {
        Self::Default
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OutputAudioConfig {
    pub format: Option<AudioFormat>,
    pub voice: Option<Voice>,
    pub speed: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InputAudioTranscription {
    pub model: Option<String>,
    pub language: Option<String>,
    pub prompt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TurnDetection {
    ServerVad {
        threshold: Option<f32>,
        prefix_padding_ms: Option<u32>,
        silence_duration_ms: Option<u32>,
        idle_timeout_ms: Option<u32>,
        create_response: Option<bool>,
        interrupt_response: Option<bool>,
    },
    SemanticVad {
        eagerness: Option<Eagerness>,
        create_response: Option<bool>,
        interrupt_response: Option<bool>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Tool {
    #[serde(rename = "function")]
    Function {
        name: String,
        description: String,
        /// JSON Schema for tool parameters (intentionally untyped).
        parameters: JsonSchema,
    },
    #[serde(rename = "mcp")]
    Mcp(McpToolConfig),
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpToolConfig {
    pub server_label: String,
    pub server_url: Option<String>,
    pub connector_id: Option<String>,
    pub headers: Option<HashMap<String, String>>,
    pub authorization: Option<String>,
    #[serde(rename = "tool_names")]
    pub allowed_tools: Option<Vec<String>>,
    pub require_approval: Option<RequireApproval>,
    pub server_description: Option<String>,
}

impl McpToolConfig {
    /// # Errors
    /// Returns an error if neither `server_url` nor `connector_id` is provided.
    #[allow(clippy::result_large_err)]
    pub fn validate(&self) -> Result<(), crate::error::Error> {
        if self.server_url.is_none() && self.connector_id.is_none() {
            return Err(crate::error::Error::InvalidClientEvent(
                "mcp tool requires server_url or connector_id".to_string(),
            ));
        }
        Ok(())
    }
}


#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalMode {
    Always,
    Never,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApprovalFilter {
    #[serde(rename = "tool_names")]
    pub tool_names: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum RequireApproval {
    Mode(ApprovalMode),
    Filter(ApprovalFilter),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolChoiceMode {
    Auto,
    None,
    Required,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolChoice {
    Mode(ToolChoiceMode),
    Specific {
        #[serde(rename = "type")]
        kind: String,
        name: Option<String>,
        server_label: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolInfo {
    pub name: String,
    pub description: Option<String>,
    /// JSON Schema for MCP tool input (intentionally untyped).
    pub input_schema: Option<JsonSchema>,
    /// MCP annotations are free-form JSON (spec-defined extensions).
    pub annotations: Option<ArbitraryJson>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum McpError {
    Protocol { code: i32, message: String },
    ToolExecution { message: String },
    Http { code: i32, message: String },
    #[serde(other)]
    Unknown,
}

/// Manual (de)serialization preserves unknown variants as raw JSON while keeping
/// strong typing for known items.
#[derive(Debug, Clone)]
pub enum Item {
    Message {
        id: Option<String>,
        status: Option<ItemStatus>,
        role: Role,
        content: Vec<ContentPart>,
    },
    FunctionCall {
        id: Option<String>,
        status: Option<ItemStatus>,
        name: String,
        call_id: String,
        arguments: String,
    },
    FunctionCallOutput {
        id: Option<String>,
        call_id: String,
        output: String,
    },
    McpCall {
        id: Option<String>,
        status: Option<ItemStatus>,
        call_id: String,
        server_label: String,
        name: String,
        arguments: String,
        approval_request_id: Option<String>,
        output: Option<String>,
        error: Option<McpError>,
    },
    McpListTools {
        id: Option<String>,
        status: Option<ItemStatus>,
        server_label: String,
        tools: Option<Vec<McpToolInfo>>,
    },
    McpApprovalRequest {
        id: Option<String>,
        status: Option<ItemStatus>,
        server_label: String,
        name: String,
        arguments: String,
    },
    McpApprovalResponse {
        id: Option<String>,
        status: Option<ItemStatus>,
        approval_request_id: String,
        approve: bool,
        reason: Option<String>,
    },
    Unknown(ArbitraryJson),
}

impl std::fmt::Display for Item {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            Self::Message { .. } => "message",
            Self::FunctionCall { .. } => "function_call",
            Self::FunctionCallOutput { .. } => "function_call_output",
            Self::McpCall { .. } => "mcp_call",
            Self::McpListTools { .. } => "mcp_list_tools",
            Self::McpApprovalRequest { .. } => "mcp_approval_request",
            Self::McpApprovalResponse { .. } => "mcp_approval_response",
            Self::Unknown(_) => "unknown",
        };
        write!(f, "{label}")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ItemRepr {
    Message {
        id: Option<String>,
        status: Option<ItemStatus>,
        role: Role,
        content: Vec<ContentPart>,
    },
    FunctionCall {
        id: Option<String>,
        status: Option<ItemStatus>,
        name: String,
        call_id: String,
        arguments: String,
    },
    FunctionCallOutput {
        id: Option<String>,
        call_id: String,
        output: String,
    },
    McpCall {
        id: Option<String>,
        status: Option<ItemStatus>,
        call_id: String,
        server_label: String,
        name: String,
        arguments: String,
        approval_request_id: Option<String>,
        output: Option<String>,
        error: Option<McpError>,
    },
    McpListTools {
        id: Option<String>,
        status: Option<ItemStatus>,
        server_label: String,
        tools: Option<Vec<McpToolInfo>>,
    },
    McpApprovalRequest {
        id: Option<String>,
        status: Option<ItemStatus>,
        server_label: String,
        name: String,
        arguments: String,
    },
    McpApprovalResponse {
        id: Option<String>,
        status: Option<ItemStatus>,
        approval_request_id: String,
        approve: bool,
        reason: Option<String>,
    },
}

impl From<ItemRepr> for Item {
    fn from(repr: ItemRepr) -> Self {
        match repr {
            ItemRepr::Message { id, status, role, content } => Self::Message { id, status, role, content },
            ItemRepr::FunctionCall { id, status, name, call_id, arguments } => Self::FunctionCall { id, status, name, call_id, arguments },
            ItemRepr::FunctionCallOutput { id, call_id, output } => Self::FunctionCallOutput { id, call_id, output },
            ItemRepr::McpCall { id, status, call_id, server_label, name, arguments, approval_request_id, output, error } => Self::McpCall {
                id,
                status,
                call_id,
                server_label,
                name,
                arguments,
                approval_request_id,
                output,
                error,
            },
            ItemRepr::McpListTools { id, status, server_label, tools } => Self::McpListTools { id, status, server_label, tools },
            ItemRepr::McpApprovalRequest { id, status, server_label, name, arguments } => Self::McpApprovalRequest { id, status, server_label, name, arguments },
            ItemRepr::McpApprovalResponse { id, status, approval_request_id, approve, reason } => Self::McpApprovalResponse {
                id,
                status,
                approval_request_id,
                approve,
                reason,
            },
        }
    }
}

impl Serialize for Item {
    #[allow(clippy::too_many_lines)]
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Unknown(value) => value.serialize(serializer),
            Self::Message { id, status, role, content } => {
                let mut state = serializer.serialize_struct("Item", 5)?;
                state.serialize_field("type", "message")?;
                if let Some(value) = id {
                    state.serialize_field("id", value)?;
                }
                if let Some(value) = status {
                    state.serialize_field("status", value)?;
                }
                state.serialize_field("role", role)?;
                state.serialize_field("content", content)?;
                state.end()
            }
            Self::FunctionCall { id, status, name, call_id, arguments } => {
                let mut state = serializer.serialize_struct("Item", 6)?;
                state.serialize_field("type", "function_call")?;
                if let Some(value) = id {
                    state.serialize_field("id", value)?;
                }
                if let Some(value) = status {
                    state.serialize_field("status", value)?;
                }
                state.serialize_field("name", name)?;
                state.serialize_field("call_id", call_id)?;
                state.serialize_field("arguments", arguments)?;
                state.end()
            }
            Self::FunctionCallOutput { id, call_id, output } => {
                let mut state = serializer.serialize_struct("Item", 4)?;
                state.serialize_field("type", "function_call_output")?;
                if let Some(value) = id {
                    state.serialize_field("id", value)?;
                }
                state.serialize_field("call_id", call_id)?;
                state.serialize_field("output", output)?;
                state.end()
            }
            Self::McpCall { id, status, call_id, server_label, name, arguments, approval_request_id, output, error } => {
                let mut state = serializer.serialize_struct("Item", 9)?;
                state.serialize_field("type", "mcp_call")?;
                if let Some(value) = id {
                    state.serialize_field("id", value)?;
                }
                if let Some(value) = status {
                    state.serialize_field("status", value)?;
                }
                state.serialize_field("call_id", call_id)?;
                state.serialize_field("server_label", server_label)?;
                state.serialize_field("name", name)?;
                state.serialize_field("arguments", arguments)?;
                if let Some(value) = approval_request_id {
                    state.serialize_field("approval_request_id", value)?;
                }
                if let Some(value) = output {
                    state.serialize_field("output", value)?;
                }
                if let Some(value) = error {
                    state.serialize_field("error", value)?;
                }
                state.end()
            }
            Self::McpListTools { id, status, server_label, tools } => {
                let mut state = serializer.serialize_struct("Item", 5)?;
                state.serialize_field("type", "mcp_list_tools")?;
                if let Some(value) = id {
                    state.serialize_field("id", value)?;
                }
                if let Some(value) = status {
                    state.serialize_field("status", value)?;
                }
                state.serialize_field("server_label", server_label)?;
                if let Some(value) = tools {
                    state.serialize_field("tools", value)?;
                }
                state.end()
            }
            Self::McpApprovalRequest { id, status, server_label, name, arguments } => {
                let mut state = serializer.serialize_struct("Item", 6)?;
                state.serialize_field("type", "mcp_approval_request")?;
                if let Some(value) = id {
                    state.serialize_field("id", value)?;
                }
                if let Some(value) = status {
                    state.serialize_field("status", value)?;
                }
                state.serialize_field("server_label", server_label)?;
                state.serialize_field("name", name)?;
                state.serialize_field("arguments", arguments)?;
                state.end()
            }
            Self::McpApprovalResponse { id, status, approval_request_id, approve, reason } => {
                let mut state = serializer.serialize_struct("Item", 6)?;
                state.serialize_field("type", "mcp_approval_response")?;
                if let Some(value) = id {
                    state.serialize_field("id", value)?;
                }
                if let Some(value) = status {
                    state.serialize_field("status", value)?;
                }
                state.serialize_field("approval_request_id", approval_request_id)?;
                state.serialize_field("approve", approve)?;
                if let Some(value) = reason {
                    state.serialize_field("reason", value)?;
                }
                state.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for Item {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = ArbitraryJson::deserialize(deserializer)?;
        match ItemRepr::deserialize(value.clone()) {
            Ok(repr) => Ok(repr.into()),
            Err(err) => {
                tracing::debug!("Failed to parse Item: {err}");
                Ok(Self::Unknown(value))
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AudioPartFormat {
    Label(String),
    Config(AudioFormat),
}

/// Manual (de)serialization preserves unknown variants as raw JSON while keeping
/// strong typing for known parts.
#[derive(Debug, Clone)]
pub enum ContentPart {
    InputText { text: String },
    InputAudio {
        audio: String,
        transcript: Option<String>,
        format: Option<AudioFormat>,
    },
    InputImage {
        image_url: String,
        detail: Option<String>,
    },
    OutputText { text: String },
    OutputAudio {
        audio: Option<String>,
        transcript: Option<String>,
        format: Option<AudioFormat>,
    },
    Text { text: String },
    Audio {
        audio: Option<String>,
        transcript: Option<String>,
        format: Option<AudioPartFormat>,
    },
    Unknown(ArbitraryJson),
}

impl std::fmt::Display for ContentPart {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            Self::InputText { .. } => "input_text",
            Self::InputAudio { .. } => "input_audio",
            Self::InputImage { .. } => "input_image",
            Self::OutputText { .. } => "output_text",
            Self::OutputAudio { .. } => "output_audio",
            Self::Text { .. } => "text",
            Self::Audio { .. } => "audio",
            Self::Unknown(_) => "unknown",
        };
        write!(f, "{label}")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ContentPartRepr {
    #[serde(rename = "input_text")]
    InputText { text: String },
    #[serde(rename = "input_audio")]
    InputAudio {
        audio: String,
        transcript: Option<String>,
        format: Option<AudioFormat>,
    },
    #[serde(rename = "input_image")]
    InputImage {
        image_url: String,
        detail: Option<String>,
    },
    #[serde(rename = "output_text")]
    OutputText { text: String },
    #[serde(rename = "output_audio")]
    OutputAudio {
        #[serde(skip_serializing_if = "Option::is_none")]
        audio: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        transcript: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        format: Option<AudioFormat>,
    },
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "audio")]
    Audio {
        audio: Option<String>,
        transcript: Option<String>,
        format: Option<AudioPartFormat>,
    },
}

impl From<ContentPartRepr> for ContentPart {
    fn from(repr: ContentPartRepr) -> Self {
        match repr {
            ContentPartRepr::InputText { text } => Self::InputText { text },
            ContentPartRepr::InputAudio { audio, transcript, format } => Self::InputAudio { audio, transcript, format },
            ContentPartRepr::InputImage { image_url, detail } => Self::InputImage { image_url, detail },
            ContentPartRepr::OutputText { text } => Self::OutputText { text },
            ContentPartRepr::OutputAudio { audio, transcript, format } => Self::OutputAudio { audio, transcript, format },
            ContentPartRepr::Text { text } => Self::Text { text },
            ContentPartRepr::Audio { audio, transcript, format } => Self::Audio { audio, transcript, format },
        }
    }
}

impl Serialize for ContentPart {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Unknown(value) => value.serialize(serializer),
            Self::InputText { text } => {
                let mut state = serializer.serialize_struct("ContentPart", 2)?;
                state.serialize_field("type", "input_text")?;
                state.serialize_field("text", text)?;
                state.end()
            }
            Self::InputAudio { audio, transcript, format } => {
                let mut state = serializer.serialize_struct("ContentPart", 4)?;
                state.serialize_field("type", "input_audio")?;
                state.serialize_field("audio", audio)?;
                if let Some(value) = transcript {
                    state.serialize_field("transcript", value)?;
                }
                if let Some(value) = format {
                    state.serialize_field("format", value)?;
                }
                state.end()
            }
            Self::InputImage { image_url, detail } => {
                let mut state = serializer.serialize_struct("ContentPart", 3)?;
                state.serialize_field("type", "input_image")?;
                state.serialize_field("image_url", image_url)?;
                if let Some(value) = detail {
                    state.serialize_field("detail", value)?;
                }
                state.end()
            }
            Self::OutputText { text } => {
                let mut state = serializer.serialize_struct("ContentPart", 2)?;
                state.serialize_field("type", "output_text")?;
                state.serialize_field("text", text)?;
                state.end()
            }
            Self::OutputAudio { audio, transcript, format } => {
                let mut state = serializer.serialize_struct("ContentPart", 4)?;
                state.serialize_field("type", "output_audio")?;
                if let Some(value) = audio {
                    state.serialize_field("audio", value)?;
                }
                if let Some(value) = transcript {
                    state.serialize_field("transcript", value)?;
                }
                if let Some(value) = format {
                    state.serialize_field("format", value)?;
                }
                state.end()
            }
            Self::Text { text } => {
                let mut state = serializer.serialize_struct("ContentPart", 2)?;
                state.serialize_field("type", "text")?;
                state.serialize_field("text", text)?;
                state.end()
            }
            Self::Audio { audio, transcript, format } => {
                let mut state = serializer.serialize_struct("ContentPart", 4)?;
                state.serialize_field("type", "audio")?;
                if let Some(value) = audio {
                    state.serialize_field("audio", value)?;
                }
                if let Some(value) = transcript {
                    state.serialize_field("transcript", value)?;
                }
                if let Some(value) = format {
                    state.serialize_field("format", value)?;
                }
                state.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for ContentPart {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = ArbitraryJson::deserialize(deserializer)?;
        match ContentPartRepr::deserialize(value.clone()) {
            Ok(repr) => Ok(repr.into()),
            Err(err) => {
                tracing::debug!("Failed to parse ContentPart: {err}");
                Ok(Self::Unknown(value))
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ResponseConfig {
    pub conversation: Option<ConversationMode>,
    /// Free-form metadata for the response.
    pub metadata: Option<Metadata>,
    pub modalities: Option<Vec<Modality>>,
    pub output_modalities: Option<OutputModalities>,
    pub input: Option<Vec<InputItem>>,
    pub instructions: Option<String>,
    pub audio: Option<AudioConfig>,
    pub voice: Option<Voice>,
    pub temperature: Option<Temperature>,
    pub max_output_tokens: Option<MaxTokens>,
    pub tools: Option<Vec<Tool>>,
    pub tool_choice: Option<ToolChoice>,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InputItem {
    ItemReference { id: String },
    Message {
        id: Option<String>,
        role: Role,
        content: Vec<ContentPart>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub total_tokens: u32,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub input_token_details: Option<InputTokenDetails>,
    pub output_token_details: Option<OutputTokenDetails>,
    pub cached_tokens: Option<u32>,
    pub cached_tokens_details: Option<CachedTokenDetails>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputTokenDetails {
    pub cached_tokens: Option<u32>,
    pub text_tokens: Option<u32>,
    pub audio_tokens: Option<u32>,
    pub image_tokens: Option<u32>,
    pub cached_tokens_details: Option<CachedTokenDetails>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputTokenDetails {
    pub text_tokens: Option<u32>,
    pub audio_tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedTokenDetails {
    pub text_tokens: Option<u32>,
    pub audio_tokens: Option<u32>,
    pub image_tokens: Option<u32>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResponseStatus {
    InProgress,
    Completed,
    Cancelled,
    Failed,
    Incomplete,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub id: String,
    pub object: String,
    pub conversation_id: Option<String>,
    pub status: ResponseStatus,
    pub status_details: Option<ResponseStatusDetails>,
    pub output: Option<Vec<Item>>,
    pub output_modalities: Option<OutputModalities>,
    pub max_output_tokens: Option<MaxTokens>,
    pub audio: Option<AudioConfig>,
    /// Free-form metadata for the response.
    pub metadata: Option<Metadata>,
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResponseStatusDetails {
    pub reason: Option<String>,
    pub error: Option<crate::error::ServerError>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_max_tokens_infinite() {
        let inf = MaxTokens::Infinite(Infinite::Inf);
        let serialized = serde_json::to_string(&inf).unwrap();
        assert_eq!(serialized, "\"inf\"");
        let deserialized: MaxTokens = serde_json::from_str(&serialized).unwrap();
        assert!(matches!(deserialized, MaxTokens::Infinite(Infinite::Inf)));
    }
}
