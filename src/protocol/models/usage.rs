use serde::{Deserialize, Serialize};

/// Billing for a single transcription, independently of any voice response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TranscriptionUsage {
    Tokens(Usage),
    Duration { seconds: f64 },
}

/// A language the transcription model detected in a committed audio turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptionLanguage {
    pub code: String,
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
