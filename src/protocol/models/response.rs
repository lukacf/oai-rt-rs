use serde::{Deserialize, Serialize};

use super::{
    AudioConfig, Item, MaxTokens, Metadata, OutputModalities, Temperature, Tool, ToolChoice, Voice,
};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ConversationMode {
    #[default]
    Auto,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ResponseConfig {
    pub conversation: Option<ConversationMode>,
    /// Free-form metadata for the response.
    pub metadata: Option<Metadata>,
    pub modalities: Option<Vec<super::Modality>>,
    pub output_modalities: Option<OutputModalities>,
    pub input_audio_format: Option<super::AudioFormat>,
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
    ItemReference {
        id: String,
    },
    Message {
        id: Option<String>,
        role: super::Role,
        content: Vec<super::ContentPart>,
    },
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
    pub usage: Option<super::Usage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResponseStatusDetails {
    pub reason: Option<String>,
    pub error: Option<crate::error::ServerError>,
}
