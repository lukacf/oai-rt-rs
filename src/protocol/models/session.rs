use serde::{Deserialize, Serialize};

use super::{
    AudioConfig, AudioFormat, InputAudioTranscription, MaxTokens, Modality, OutputModalities,
    PromptRef, Temperature, Tool, ToolChoice, Voice,
};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SessionKind {
    #[default]
    Realtime,
    Transcription,
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
    pub metadata: Option<super::Metadata>,
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
    pub turn_detection: Option<super::TurnDetection>,
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
    pub turn_detection: Option<super::TurnDetection>,
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
