use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize};

use super::{
    AudioConfig, AudioFormat, InputAudioTranscription, MaxTokens, Modality, NoiseReduction,
    Nullable, OutputModalities, PromptRef, ReasoningConfig, Temperature, Tool, ToolChoice,
    TurnDetection, Voice,
};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SessionKind {
    #[default]
    Realtime,
    Transcription,
    Translation,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modalities: Option<Vec<Modality>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<PromptRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncation: Option<Truncation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_audio_format: Option<AudioFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_audio_format: Option<AudioFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_audio_transcription: Option<Nullable<InputAudioTranscription>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_detection: Option<Nullable<TurnDetection>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<Temperature>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<MaxTokens>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio: Option<AudioConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tracing: Option<Tracing>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub voice: Option<Voice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ReasoningConfig>,
}

impl SessionConfig {
    pub(crate) fn warn_if_retiring_models(&self) {
        let nested = self
            .audio
            .as_ref()
            .and_then(|audio| audio.input.as_ref())
            .and_then(|input| input.transcription.as_ref());
        let transcription = nested.or(self.input_audio_transcription.as_ref());
        if self.kind == SessionKind::Transcription {
            if !matches!(transcription, Some(Nullable::Null)) {
                let model = transcription
                    .and_then(Nullable::as_ref)
                    .and_then(|config| config.model.as_deref())
                    .unwrap_or(&self.model);
                super::common::warn_if_retiring_model(model);
            }
        } else {
            super::common::warn_if_retiring_model(&self.model);
            super::audio::warn_if_retiring_transcription(transcription);
        }
    }

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
            reasoning: None,
        }
    }

    // Both GA client secrets and session.update use the same input-only shape.
    pub(crate) fn transcription_update(&self) -> SessionUpdate {
        let mut input = self
            .audio
            .as_ref()
            .and_then(|audio| audio.input.clone())
            .unwrap_or_default();
        input.format = input.format.or_else(|| self.input_audio_format.clone());
        input.turn_detection = input.turn_detection.or_else(|| self.turn_detection.clone());
        let transcription = input.transcription.get_or_insert_with(|| {
            self.input_audio_transcription
                .clone()
                .unwrap_or_else(|| Nullable::Value(InputAudioTranscription::default()))
        });
        if let Nullable::Value(transcription) = transcription {
            transcription
                .model
                .get_or_insert_with(|| self.model.clone());
        }
        SessionUpdate {
            config: SessionUpdateConfig {
                kind: Some(SessionKind::Transcription),
                audio: Some(AudioConfig {
                    input: Some(input),
                    output: None,
                }),
                include: self.include.clone(),
                ..SessionUpdateConfig::default()
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionUpdateConfig {
    /// Partial updates only; GA forbids changing `model` or session `type`.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub kind: Option<SessionKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_modalities: Option<OutputModalities>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modalities: Option<Vec<Modality>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<PromptRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncation: Option<Truncation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_audio_format: Option<AudioFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_audio_format: Option<AudioFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_audio_transcription: Option<Nullable<InputAudioTranscription>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_detection: Option<Nullable<TurnDetection>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<Temperature>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<MaxTokens>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio: Option<AudioConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tracing: Option<Tracing>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub voice: Option<Voice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ReasoningConfig>,
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

/// Input-only session snapshot returned by GA transcription events and client secrets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionSession {
    pub id: String,
    pub object: String,
    #[serde(rename = "type")]
    kind: TranscriptionKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio: Option<AudioConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
enum TranscriptionKind {
    #[serde(rename = "transcription")]
    Transcription,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct SessionUpdate {
    /// Flattened to match the API's session.update JSON shape.
    #[serde(flatten)]
    pub config: SessionUpdateConfig,
}

/// Flat payload for the legacy `transcription_session.update` event.
/// Use [`SessionUpdate`] with [`SessionKind::Transcription`] for GA.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TranscriptionSessionUpdateConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_audio_format: Option<AudioFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_audio_transcription: Option<Nullable<InputAudioTranscription>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_detection: Option<Nullable<TurnDetection>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_audio_noise_reduction: Option<Nullable<NoiseReduction>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include: Option<Vec<String>>,
}

impl From<&SessionConfig> for TranscriptionSessionUpdateConfig {
    fn from(config: &SessionConfig) -> Self {
        let input = config.audio.as_ref().and_then(|audio| audio.input.as_ref());
        Self {
            input_audio_format: input.and_then(|input| input.format.clone()),
            input_audio_transcription: input.and_then(|input| input.transcription.clone()),
            turn_detection: input.and_then(|input| input.turn_detection.clone()),
            input_audio_noise_reduction: input.and_then(|input| input.noise_reduction.clone()),
            include: config.include.clone(),
        }
    }
}

impl Serialize for SessionUpdate {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut map = serializer.serialize_map(None)?;
        let value = serde_json::to_value(&self.config).map_err(serde::ser::Error::custom)?;
        if let serde_json::Value::Object(obj) = value {
            if !obj.contains_key("type") {
                map.serialize_entry("type", "realtime")?;
            }
            for (k, v) in obj {
                map.serialize_entry(&k, &v)?;
            }
        }
        map.end()
    }
}
