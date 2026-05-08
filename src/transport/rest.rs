use crate::error::Result;
use crate::protocol::models::{
    AudioConfig, AudioFormat, InputAudioTranscription, MaxTokens, Modality, NoiseReduction,
    Nullable, PromptRef, ReasoningConfig, Session, SessionConfig, SessionKind, Temperature, Tool,
    ToolChoice, Tracing, Truncation, TurnDetection, Voice,
};
use reqwest::{
    Client, RequestBuilder,
    header::{AUTHORIZATION, HeaderValue, LOCATION},
    multipart,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EphemeralSecretResponse {
    pub value: String,
    pub expires_at: u64,
    pub session: Session,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslationSession {
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<SessionKind>,
    pub expires_at: Option<u64>,
    pub model: String,
    pub audio: Option<AudioConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslationClientSecretResponse {
    pub value: String,
    pub expires_at: u64,
    pub session: TranslationSession,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RealtimeSessionResponse {
    pub id: String,
    pub object: String,
    pub client_secret: Option<ClientSecret>,
    #[serde(flatten)]
    pub payload: HashMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionSessionResponse {
    pub id: String,
    pub object: String,
    pub client_secret: Option<ClientSecret>,
    #[serde(flatten)]
    pub payload: HashMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpiresAfter {
    pub anchor: String,
    pub seconds: u64,
}

#[derive(Debug, Clone, Serialize)]
struct CreateClientSecretRequest {
    pub session: SessionConfig,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_after: Option<ExpiresAfter>,
}

#[derive(Debug, Clone, Serialize)]
struct CreateTranslationClientSecretRequest {
    pub session: TranslationClientSecretSession,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_after: Option<ExpiresAfter>,
}

#[derive(Debug, Clone, Serialize)]
struct CreateRealtimeSessionRequest {
    #[serde(rename = "type")]
    pub kind: SessionKind,
    pub model: String,
    pub modalities: Vec<Modality>,
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

#[derive(Debug, Clone, Serialize)]
struct CreateTranscriptionSessionRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_audio_format: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_audio_transcription: Option<Nullable<InputAudioTranscription>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_detection: Option<Nullable<TurnDetection>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_audio_noise_reduction: Option<Nullable<NoiseReduction>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include: Option<Vec<String>>,
}

impl From<&SessionConfig> for CreateTranscriptionSessionRequest {
    fn from(session: &SessionConfig) -> Self {
        let input = session
            .audio
            .as_ref()
            .and_then(|audio| audio.input.as_ref());
        Self {
            input_audio_format: input
                .and_then(|input| input.format.as_ref())
                .map(transcription_audio_format_label),
            input_audio_transcription: input.and_then(|input| input.transcription.clone()),
            turn_detection: input.and_then(|input| input.turn_detection.clone()),
            input_audio_noise_reduction: input.and_then(|input| input.noise_reduction.clone()),
            include: session.include.clone(),
        }
    }
}

const fn transcription_audio_format_label(format: &AudioFormat) -> &'static str {
    match format {
        AudioFormat::Pcm { .. } => "pcm16",
        AudioFormat::Pcmu => "g711_ulaw",
        AudioFormat::Pcma => "g711_alaw",
    }
}

impl From<SessionConfig> for CreateRealtimeSessionRequest {
    fn from(session: SessionConfig) -> Self {
        Self {
            kind: session.kind,
            model: session.model,
            modalities: session
                .modalities
                .unwrap_or_else(|| match session.output_modalities {
                    crate::protocol::models::OutputModalities::Audio => {
                        vec![Modality::Audio, Modality::Text]
                    }
                    other => other.as_modalities(),
                }),
            include: session.include,
            prompt: session.prompt,
            truncation: session.truncation,
            instructions: session.instructions,
            input_audio_format: session.input_audio_format,
            output_audio_format: session.output_audio_format,
            input_audio_transcription: session.input_audio_transcription,
            turn_detection: session.turn_detection,
            tools: session.tools,
            tool_choice: session.tool_choice,
            temperature: session.temperature,
            max_output_tokens: session.max_output_tokens,
            audio: session.audio,
            tracing: session.tracing,
            voice: session.voice,
            reasoning: session.reasoning,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct TranslationClientSecretSession {
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio: Option<AudioConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include: Option<Vec<String>>,
}

impl From<SessionConfig> for TranslationClientSecretSession {
    fn from(session: SessionConfig) -> Self {
        Self {
            model: session.model,
            audio: session.audio,
            include: session.include,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallCreationResponse {
    pub sdp: String,
    pub call_id: Option<String>,
}

const BASE_URL: &str = "https://api.openai.com/v1/realtime";
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_POOL_IDLE_TIMEOUT: Duration = Duration::from_secs(90);

/// An adapter for the `OpenAI` Realtime REST API.
#[derive(Clone, Debug)]
pub struct RealtimeRestAdapter {
    client: Client,
    auth_header: HeaderValue,
}

impl RealtimeRestAdapter {
    /// Create a new adapter with the given API key.
    ///
    /// # Errors
    /// Returns an error if the API key results in an invalid header or client build fails.
    #[allow(clippy::result_large_err)]
    pub fn new(api_key: &str) -> Result<Self> {
        Self::new_with_timeouts(api_key, DEFAULT_TIMEOUT, DEFAULT_POOL_IDLE_TIMEOUT)
    }

    /// Create a new adapter with custom timeouts.
    ///
    /// # Errors
    /// Returns an error if the API key results in an invalid header or client build fails.
    #[allow(clippy::result_large_err)]
    pub fn new_with_timeouts(
        api_key: &str,
        timeout: Duration,
        pool_idle_timeout: Duration,
    ) -> Result<Self> {
        let client = Client::builder()
            .timeout(timeout)
            .pool_idle_timeout(pool_idle_timeout)
            .build()?;

        let auth_header = HeaderValue::from_str(&format!("Bearer {api_key}"))?;

        Ok(Self {
            client,
            auth_header,
        })
    }

    /// Create an ephemeral client secret for browser usage (GA).
    ///
    /// # Errors
    /// Returns an error if the HTTP request fails.
    pub async fn create_client_secret(
        &self,
        session: SessionConfig,
    ) -> Result<EphemeralSecretResponse> {
        self.create_client_secret_with_expiry(session, None).await
    }

    /// Create an ephemeral client secret with an explicit expiry configuration.
    ///
    /// # Errors
    /// Returns an error if the HTTP request fails.
    pub async fn create_client_secret_with_expiry(
        &self,
        session: SessionConfig,
        expires_after: Option<ExpiresAfter>,
    ) -> Result<EphemeralSecretResponse> {
        self.create_client_secret_with_expiry_and_safety_identifier(session, expires_after, None)
            .await
    }

    /// Create an ephemeral client secret with optional expiry and safety identifier.
    ///
    /// # Errors
    /// Returns an error if the HTTP request fails.
    pub async fn create_client_secret_with_expiry_and_safety_identifier(
        &self,
        session: SessionConfig,
        expires_after: Option<ExpiresAfter>,
        safety_identifier: Option<&str>,
    ) -> Result<EphemeralSecretResponse> {
        if session.kind != SessionKind::Realtime {
            return Err(crate::error::Error::InvalidClientEvent(
                "client_secrets only supports realtime sessions".to_string(),
            ));
        }

        let req = self
            .authorized(self.client.post(format!("{BASE_URL}/client_secrets")))
            .json(&CreateClientSecretRequest {
                session,
                expires_after,
            });
        let res = Self::with_safety_identifier(req, safety_identifier)?
            .send()
            .await?
            .error_for_status()?;

        Ok(res.json().await?)
    }

    /// Create a Realtime session through `/v1/realtime/sessions`.
    ///
    /// # Errors
    /// Returns an error if the HTTP request fails.
    pub async fn create_session(&self, session: SessionConfig) -> Result<RealtimeSessionResponse> {
        self.create_session_with_safety_identifier(session, None)
            .await
    }

    /// Create a Realtime session with optional safety identifier.
    ///
    /// # Errors
    /// Returns an error if the HTTP request fails.
    pub async fn create_session_with_safety_identifier(
        &self,
        session: SessionConfig,
        safety_identifier: Option<&str>,
    ) -> Result<RealtimeSessionResponse> {
        if session.kind != SessionKind::Realtime {
            return Err(crate::error::Error::InvalidClientEvent(
                "sessions only supports realtime sessions".to_string(),
            ));
        }

        let req = self
            .authorized(self.client.post(format!("{BASE_URL}/sessions")))
            .json(&CreateRealtimeSessionRequest::from(session));
        let res = Self::with_safety_identifier(req, safety_identifier)?
            .send()
            .await?
            .error_for_status()?;

        Ok(res.json().await?)
    }

    /// Create a translation client secret for browser usage.
    ///
    /// # Errors
    /// Returns an error if the HTTP request fails.
    pub async fn create_translation_client_secret(
        &self,
        session: SessionConfig,
    ) -> Result<TranslationClientSecretResponse> {
        self.create_translation_client_secret_with_expiry_and_safety_identifier(session, None, None)
            .await
    }

    /// Create a translation client secret with optional expiry and safety identifier.
    ///
    /// # Errors
    /// Returns an error if the HTTP request fails.
    pub async fn create_translation_client_secret_with_expiry_and_safety_identifier(
        &self,
        session: SessionConfig,
        expires_after: Option<ExpiresAfter>,
        safety_identifier: Option<&str>,
    ) -> Result<TranslationClientSecretResponse> {
        if session.kind != SessionKind::Translation {
            return Err(crate::error::Error::InvalidClientEvent(
                "translation client_secrets only supports translation sessions".to_string(),
            ));
        }

        let req = self
            .authorized(
                self.client
                    .post(format!("{BASE_URL}/translations/client_secrets")),
            )
            .json(&CreateTranslationClientSecretRequest {
                session: session.into(),
                expires_after,
            });
        let res = Self::with_safety_identifier(req, safety_identifier)?
            .send()
            .await?
            .error_for_status()?;

        Ok(res.json().await?)
    }

    /// Create an ephemeral transcription session for browser usage.
    ///
    /// # Errors
    /// Returns an error if the HTTP request fails.
    pub async fn create_transcription_session(
        &self,
        session: SessionConfig,
    ) -> Result<TranscriptionSessionResponse> {
        self.create_transcription_session_with_safety_identifier(session, None)
            .await
    }

    /// Create an ephemeral transcription session with an optional safety identifier.
    ///
    /// # Errors
    /// Returns an error if the HTTP request fails.
    pub async fn create_transcription_session_with_safety_identifier(
        &self,
        session: SessionConfig,
        safety_identifier: Option<&str>,
    ) -> Result<TranscriptionSessionResponse> {
        if session.kind != SessionKind::Transcription {
            return Err(crate::error::Error::InvalidClientEvent(
                "transcription_sessions only supports transcription sessions".to_string(),
            ));
        }

        let req = self
            .authorized(
                self.client
                    .post(format!("{BASE_URL}/transcription_sessions")),
            )
            .json(&CreateTranscriptionSessionRequest::from(&session));
        let res = Self::with_safety_identifier(req, safety_identifier)?
            .send()
            .await?
            .error_for_status()?;

        Ok(res.json().await?)
    }

    /// Post an SDP offer to initiate a WebRTC call (Direct - raw SDP).
    ///
    /// # Errors
    /// Returns an error if the HTTP request fails.
    pub async fn post_sdp_offer_raw(&self, sdp_offer: String) -> Result<String> {
        Ok(self.post_sdp_offer_raw_with_call_id(sdp_offer).await?.sdp)
    }

    /// Post an SDP offer to initiate a WebRTC call (Direct - raw SDP) and return `call_id`.
    ///
    /// # Errors
    /// Returns an error if the HTTP request fails.
    pub async fn post_sdp_offer_raw_with_call_id(
        &self,
        sdp_offer: String,
    ) -> Result<CallCreationResponse> {
        self.post_sdp_offer_raw_with_call_id_and_safety_identifier(sdp_offer, None)
            .await
    }

    /// Post an SDP offer to initiate a WebRTC call with optional safety identifier.
    ///
    /// # Errors
    /// Returns an error if the HTTP request fails.
    pub async fn post_sdp_offer_raw_with_call_id_and_safety_identifier(
        &self,
        sdp_offer: String,
        safety_identifier: Option<&str>,
    ) -> Result<CallCreationResponse> {
        let url = format!("{BASE_URL}/calls");

        let req = self
            .authorized(self.client.post(url))
            .header("Content-Type", "application/sdp")
            .body(sdp_offer);
        let res = Self::with_safety_identifier(req, safety_identifier)?
            .send()
            .await?
            .error_for_status()?;

        let call_id = res.headers().get(LOCATION).and_then(extract_call_id);
        Ok(CallCreationResponse {
            sdp: res.text().await?,
            call_id,
        })
    }

    /// Post an SDP offer to initiate a WebRTC call (Unified - Multipart).
    ///
    /// # Errors
    /// Returns an error if the HTTP request fails.
    pub async fn post_sdp_offer_multipart(
        &self,
        sdp_offer: String,
        session: Option<SessionConfig>,
    ) -> Result<String> {
        Ok(self
            .post_sdp_offer_multipart_with_call_id(sdp_offer, session)
            .await?
            .sdp)
    }

    /// Post an SDP offer to initiate a WebRTC call (Unified - Multipart) and return `call_id`.
    ///
    /// # Errors
    /// Returns an error if the HTTP request fails.
    pub async fn post_sdp_offer_multipart_with_call_id(
        &self,
        sdp_offer: String,
        session: Option<SessionConfig>,
    ) -> Result<CallCreationResponse> {
        self.post_sdp_offer_multipart_with_call_id_and_safety_identifier(sdp_offer, session, None)
            .await
    }

    /// Post an SDP offer to initiate a WebRTC call with optional safety identifier.
    ///
    /// # Errors
    /// Returns an error if the HTTP request fails.
    pub async fn post_sdp_offer_multipart_with_call_id_and_safety_identifier(
        &self,
        sdp_offer: String,
        session: Option<SessionConfig>,
        safety_identifier: Option<&str>,
    ) -> Result<CallCreationResponse> {
        let url = format!("{BASE_URL}/calls");

        let sdp_part = multipart::Part::text(sdp_offer)
            .mime_str("application/sdp")
            .map_err(|e| crate::error::Error::Mime(e.to_string()))?;
        let mut form = multipart::Form::new().part("sdp", sdp_part);

        if let Some(s) = session {
            let session_part = multipart::Part::text(serde_json::to_string(&s)?)
                .mime_str("application/json")
                .map_err(|e| crate::error::Error::Mime(e.to_string()))?;
            form = form.part("session", session_part);
        }

        let req = self.authorized(self.client.post(url)).multipart(form);
        let res = Self::with_safety_identifier(req, safety_identifier)?
            .send()
            .await?
            .error_for_status()?;

        let call_id = res.headers().get(LOCATION).and_then(extract_call_id);
        Ok(CallCreationResponse {
            sdp: res.text().await?,
            call_id,
        })
    }

    /// Accept an incoming SIP call.
    ///
    /// # Errors
    /// Returns an error if the HTTP request fails or returns a non-success status.
    pub async fn sip_accept(&self, call_id: &str, session: SessionConfig) -> Result<()> {
        let url = format!("{BASE_URL}/calls/{call_id}/accept");

        if session.kind != SessionKind::Realtime {
            return Err(crate::error::Error::InvalidClientEvent(
                "sip.accept only supports realtime sessions".to_string(),
            ));
        }

        self.authorized(self.client.post(&url))
            .json(&session)
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    /// Reject an incoming SIP call.
    ///
    /// # Errors
    /// Returns an error if the HTTP request fails.
    pub async fn sip_reject(&self, call_id: &str) -> Result<()> {
        let url = format!("{BASE_URL}/calls/{call_id}/reject");
        self.authorized(self.client.post(&url))
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    /// Hang up a call (WebRTC or SIP).
    ///
    /// # Errors
    /// Returns an error if the HTTP request fails.
    pub async fn hangup(&self, call_id: &str) -> Result<()> {
        let url = format!("{BASE_URL}/calls/{call_id}/hangup");
        self.authorized(self.client.post(&url))
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    /// Refer (transfer) a SIP call to another URI.
    ///
    /// # Errors
    /// Returns an error if the HTTP request fails.
    pub async fn sip_refer(&self, call_id: &str, target_uri: impl Into<String>) -> Result<()> {
        let url = format!("{BASE_URL}/calls/{call_id}/refer");
        let body = SipReferRequest {
            target_uri: target_uri.into(),
        };

        self.authorized(self.client.post(&url))
            .json(&body)
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    fn authorized(&self, request: RequestBuilder) -> RequestBuilder {
        request.header(AUTHORIZATION, &self.auth_header)
    }

    #[allow(clippy::result_large_err)]
    fn with_safety_identifier(
        request: RequestBuilder,
        safety_identifier: Option<&str>,
    ) -> Result<RequestBuilder> {
        let Some(safety_identifier) = safety_identifier else {
            return Ok(request);
        };
        if safety_identifier.trim().is_empty() {
            return Err(crate::error::Error::InvalidClientEvent(
                "safety_identifier must not be empty".to_string(),
            ));
        }
        Ok(request.header(
            "OpenAI-Safety-Identifier",
            HeaderValue::from_str(safety_identifier)?,
        ))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientSecret {
    pub value: String,
    pub expires_at: u64,
}

#[derive(Debug, Clone, Serialize)]
struct SipReferRequest {
    pub target_uri: String,
}

fn extract_call_id(location: &HeaderValue) -> Option<String> {
    let value = location.to_str().ok()?;
    value
        .rsplit('/')
        .find(|segment| !segment.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::models::{
        OutputAudioConfig, OutputModalities, ReasoningEffort, ToolChoiceMode,
    };
    use serde_json::json;

    #[test]
    fn safety_identifier_header_can_be_added_to_call_requests() {
        let adapter = RealtimeRestAdapter::new("test-key").expect("adapter");
        let req = adapter
            .authorized(adapter.client.post(format!("{BASE_URL}/calls")))
            .header("Content-Type", "application/sdp")
            .body("v=0".to_string());
        let req = RealtimeRestAdapter::with_safety_identifier(req, Some("hashed-user"))
            .expect("safety header")
            .build()
            .expect("request");

        assert_eq!(req.url().path(), "/v1/realtime/calls");
        assert_eq!(
            req.headers()
                .get("OpenAI-Safety-Identifier")
                .and_then(|value| value.to_str().ok()),
            Some("hashed-user")
        );
    }

    #[test]
    fn empty_safety_identifier_is_rejected() {
        let adapter = RealtimeRestAdapter::new("test-key").expect("adapter");
        let req = adapter.authorized(adapter.client.post(format!("{BASE_URL}/calls")));
        let err = RealtimeRestAdapter::with_safety_identifier(req, Some("  "))
            .expect_err("empty safety identifier should fail");
        assert!(matches!(
            err,
            crate::error::Error::InvalidClientEvent(message)
                if message.contains("safety_identifier")
        ));
    }

    #[test]
    fn realtime_session_request_preserves_configured_fields() {
        let mut session = SessionConfig::new(
            SessionKind::Realtime,
            crate::protocol::models::GPT_REALTIME_2,
            OutputModalities::Audio,
        );
        session.instructions = Some("Use tools when useful.".to_string());
        session.audio = Some(AudioConfig {
            input: None,
            output: Some(OutputAudioConfig {
                format: None,
                voice: Some(Voice::from("marin")),
                speed: None,
                language: None,
            }),
        });
        session.tools = Some(vec![Tool::Function {
            name: "sum".to_string(),
            description: Some("Add two integers.".to_string()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "a": { "type": "integer" },
                    "b": { "type": "integer" }
                },
                "required": ["a", "b"],
                "additionalProperties": false
            }),
        }]);
        session.tool_choice = Some(ToolChoice::Mode(ToolChoiceMode::Auto));
        session.reasoning = Some(ReasoningConfig {
            effort: Some(ReasoningEffort::Low),
        });

        let serialized =
            serde_json::to_value(CreateRealtimeSessionRequest::from(session)).expect("serialize");

        assert_eq!(serialized.pointer("/type"), Some(&json!("realtime")));
        assert_eq!(
            serialized.pointer("/audio/output/voice"),
            Some(&json!("marin"))
        );
        assert_eq!(serialized.pointer("/tools/0/name"), Some(&json!("sum")));
        assert_eq!(serialized.pointer("/tool_choice"), Some(&json!("auto")));
        assert_eq!(serialized.pointer("/reasoning/effort"), Some(&json!("low")));
    }
}
