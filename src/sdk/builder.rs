use crate::protocol::models::{
    AudioConfig, AudioFormat, GPT_REALTIME_TRANSLATE, GPT_REALTIME_WHISPER, InputAudioConfig,
    InputAudioTranscription, MaxTokens, NoiseReduction, OutputAudioConfig, OutputModalities,
    ReasoningConfig, ReasoningEffort, SessionConfig, SessionKind, Temperature, ToolChoice,
    TurnDetection,
};
use crate::{Error, Result};
use std::sync::Arc;

use super::EventHandlers;
use super::session::SessionConfigSnapshot;
use super::tools::{ToolDispatcher, ToolRegistry};

pub struct Realtime;

impl Realtime {
    #[must_use]
    pub fn builder() -> RealtimeBuilder {
        RealtimeBuilder::new()
    }

    #[must_use]
    pub fn translation_builder() -> RealtimeBuilder {
        RealtimeBuilder::new().translation_session()
    }

    #[must_use]
    pub fn transcription_builder() -> RealtimeBuilder {
        RealtimeBuilder::new().transcription_session()
    }

    /// Connect via WebSocket with defaults.
    ///
    /// # Errors
    /// Returns an error if the connection fails.
    pub async fn connect_ws(api_key: &str) -> Result<super::Session> {
        RealtimeBuilder::new().api_key(api_key).connect_ws().await
    }
}

pub struct RealtimeBuilder {
    api_key: Option<String>,
    model: Option<String>,
    call_id: Option<String>,
    safety_identifier: Option<String>,
    voice: Option<String>,
    session_kind: SessionKind,
    output_modalities: Option<OutputModalities>,
    include: Vec<String>,
    instructions: Option<String>,
    tool_choice: Option<ToolChoice>,
    temperature: Option<Temperature>,
    max_output_tokens: Option<MaxTokens>,
    reasoning: Option<ReasoningConfig>,
    audio: Option<AudioConfig>,
    auto_barge_in: bool,
    auto_tool_response: bool,
    send_initial_session_update: bool,
    handlers: EventHandlers,
    tools: ToolRegistry,
    dispatcher: Option<Arc<dyn ToolDispatcher>>,
}

impl RealtimeBuilder {
    #[must_use]
    pub fn new() -> Self {
        Self {
            api_key: None,
            model: None,
            call_id: None,
            safety_identifier: None,
            voice: None,
            session_kind: SessionKind::Realtime,
            output_modalities: None,
            include: Vec::new(),
            instructions: None,
            tool_choice: None,
            temperature: None,
            max_output_tokens: None,
            reasoning: None,
            audio: None,
            auto_barge_in: false,
            auto_tool_response: true,
            send_initial_session_update: true,
            handlers: EventHandlers::new(),
            tools: ToolRegistry::new(),
            dispatcher: None,
        }
    }

    #[must_use]
    pub fn api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = Some(key.into());
        self
    }

    #[must_use]
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    #[must_use]
    pub fn call_id(mut self, call_id: impl Into<String>) -> Self {
        self.call_id = Some(call_id.into());
        self
    }

    #[must_use]
    pub fn safety_identifier(mut self, safety_identifier: impl Into<String>) -> Self {
        self.safety_identifier = Some(safety_identifier.into());
        self
    }

    #[must_use]
    pub fn voice(mut self, voice: impl Into<String>) -> Self {
        let voice = voice.into();
        self.voice = Some(voice.clone());
        let output_voice = Some(crate::protocol::models::Voice::from(voice));
        match self.audio.as_mut() {
            Some(audio) => {
                let output = audio.output.get_or_insert_with(OutputAudioConfig::default);
                output.voice = output_voice;
            }
            None => {
                self.audio = Some(AudioConfig {
                    input: None,
                    output: Some(OutputAudioConfig {
                        format: None,
                        voice: output_voice,
                        speed: None,
                        language: None,
                    }),
                });
            }
        }
        self
    }

    #[must_use]
    pub const fn session_kind(mut self, kind: SessionKind) -> Self {
        self.session_kind = kind;
        self
    }

    #[must_use]
    pub const fn transcription_session(mut self) -> Self {
        self.session_kind = SessionKind::Transcription;
        self
    }

    #[must_use]
    pub fn translation_session(mut self) -> Self {
        self.session_kind = SessionKind::Translation;
        self.model
            .get_or_insert_with(|| GPT_REALTIME_TRANSLATE.to_string());
        self.output_modalities = Some(OutputModalities::Audio);
        self
    }

    #[must_use]
    pub fn transcription_model(mut self, model: impl Into<String>) -> Self {
        let audio = self.audio.get_or_insert_with(AudioConfig::default);
        let input = audio.input.get_or_insert_with(InputAudioConfig::default);
        let transcription = input
            .transcription
            .get_or_insert_with(|| {
                crate::protocol::models::Nullable::Value(InputAudioTranscription::default())
            })
            .as_ref()
            .cloned()
            .unwrap_or_default();
        input.transcription = Some(crate::protocol::models::Nullable::Value(
            InputAudioTranscription {
                model: Some(model.into()),
                ..transcription
            },
        ));
        self
    }

    #[must_use]
    pub fn transcription_language(mut self, language: impl Into<String>) -> Self {
        let audio = self.audio.get_or_insert_with(AudioConfig::default);
        let input = audio.input.get_or_insert_with(InputAudioConfig::default);
        let transcription = input
            .transcription
            .get_or_insert_with(|| {
                crate::protocol::models::Nullable::Value(InputAudioTranscription::default())
            })
            .as_ref()
            .cloned()
            .unwrap_or_default();
        input.transcription = Some(crate::protocol::models::Nullable::Value(
            InputAudioTranscription {
                language: Some(language.into()),
                ..transcription
            },
        ));
        self
    }

    #[must_use]
    pub fn transcription_prompt(mut self, prompt: impl Into<String>) -> Self {
        let audio = self.audio.get_or_insert_with(AudioConfig::default);
        let input = audio.input.get_or_insert_with(InputAudioConfig::default);
        let transcription = input
            .transcription
            .get_or_insert_with(|| {
                crate::protocol::models::Nullable::Value(InputAudioTranscription::default())
            })
            .as_ref()
            .cloned()
            .unwrap_or_default();
        input.transcription = Some(crate::protocol::models::Nullable::Value(
            InputAudioTranscription {
                prompt: Some(prompt.into()),
                ..transcription
            },
        ));
        self
    }

    #[must_use]
    pub fn include_transcription_logprobs(self) -> Self {
        self.include("item.input_audio_transcription.logprobs")
    }

    #[must_use]
    pub fn include(mut self, field: impl Into<String>) -> Self {
        let field = field.into();
        self.session_include_mut().push(field);
        self
    }

    #[must_use]
    pub fn input_noise_reduction(mut self, noise_reduction: NoiseReduction) -> Self {
        let audio = self.audio.get_or_insert_with(AudioConfig::default);
        let input = audio.input.get_or_insert_with(InputAudioConfig::default);
        input.noise_reduction = Some(crate::protocol::models::Nullable::Value(noise_reduction));
        self
    }

    #[must_use]
    pub fn input_turn_detection(mut self, turn_detection: TurnDetection) -> Self {
        let audio = self.audio.get_or_insert_with(AudioConfig::default);
        let input = audio.input.get_or_insert_with(InputAudioConfig::default);
        input.turn_detection = Some(crate::protocol::models::Nullable::Value(turn_detection));
        self
    }

    #[must_use]
    pub fn manual_turn_detection(mut self) -> Self {
        let audio = self.audio.get_or_insert_with(AudioConfig::default);
        let input = audio.input.get_or_insert_with(InputAudioConfig::default);
        input.turn_detection = Some(crate::protocol::models::Nullable::Null);
        self
    }

    #[must_use]
    pub fn translation_language(mut self, language: impl Into<String>) -> Self {
        let audio = self.audio.get_or_insert_with(AudioConfig::default);
        let output = audio.output.get_or_insert_with(OutputAudioConfig::default);
        output.language = Some(language.into());
        self
    }

    #[must_use]
    pub fn instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = Some(instructions.into());
        self
    }

    #[must_use]
    pub fn tool_choice(mut self, choice: ToolChoice) -> Self {
        self.tool_choice = Some(choice);
        self
    }

    #[must_use]
    pub const fn temperature(mut self, temperature: Temperature) -> Self {
        self.temperature = Some(temperature);
        self
    }

    #[must_use]
    pub const fn max_output_tokens(mut self, max_output_tokens: MaxTokens) -> Self {
        self.max_output_tokens = Some(max_output_tokens);
        self
    }

    #[must_use]
    pub const fn reasoning_effort(mut self, effort: ReasoningEffort) -> Self {
        self.reasoning = Some(ReasoningConfig {
            effort: Some(effort),
        });
        self
    }

    #[must_use]
    pub const fn auto_barge_in(mut self, enabled: bool) -> Self {
        self.auto_barge_in = enabled;
        self
    }

    #[must_use]
    pub const fn auto_tool_response(mut self, enabled: bool) -> Self {
        self.auto_tool_response = enabled;
        self
    }

    #[must_use]
    pub const fn manual_sideband_control(mut self) -> Self {
        self.auto_barge_in = false;
        self.auto_tool_response = false;
        self.send_initial_session_update = false;
        self
    }

    #[must_use]
    pub fn tool_dispatcher(mut self, dispatcher: Arc<dyn ToolDispatcher>) -> Self {
        self.dispatcher = Some(dispatcher);
        self
    }

    #[must_use]
    pub fn voice_session(self) -> VoiceSessionBuilder {
        VoiceSessionBuilder::new(self)
    }

    #[must_use]
    pub const fn output_audio(mut self) -> Self {
        self.output_modalities = Some(OutputModalities::Audio);
        self
    }

    #[must_use]
    pub const fn output_text(mut self) -> Self {
        self.output_modalities = Some(OutputModalities::Text);
        self
    }

    #[must_use]
    pub const fn output_audio_text(mut self) -> Self {
        self.output_modalities = Some(OutputModalities::AudioText);
        self
    }

    #[must_use]
    pub fn tools(mut self, tools: ToolRegistry) -> Self {
        self.tools = tools;
        self
    }

    #[must_use]
    pub fn tool<TArgs, TResp, F, Fut>(mut self, name: &str, handler: F) -> Self
    where
        TArgs: schemars::JsonSchema + serde::de::DeserializeOwned + Send + 'static,
        TResp: serde::Serialize + Send + 'static,
        F: Fn(TArgs) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<TResp>> + Send + 'static,
    {
        self.tools.tool(name, handler);
        self
    }

    #[must_use]
    pub fn tool_desc<TArgs, TResp, F, Fut>(
        mut self,
        name: &str,
        description: impl Into<String>,
        handler: F,
    ) -> Self
    where
        TArgs: schemars::JsonSchema + serde::de::DeserializeOwned + Send + 'static,
        TResp: serde::Serialize + Send + 'static,
        F: Fn(TArgs) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<TResp>> + Send + 'static,
    {
        self.tools.tool_desc(name, description, handler);
        self
    }

    #[must_use]
    pub fn tool_with_description<TArgs, TResp, F, Fut>(
        mut self,
        name: &str,
        description: impl Into<String>,
        handler: F,
    ) -> Self
    where
        TArgs: schemars::JsonSchema + serde::de::DeserializeOwned + Send + 'static,
        TResp: serde::Serialize + Send + 'static,
        F: Fn(TArgs) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<TResp>> + Send + 'static,
    {
        self.tools.tool_with_description(name, description, handler);
        self
    }

    /// # Errors
    /// Returns an error if the MCP tool configuration is invalid.
    // Keep a single public error type for the SDK surface.
    #[allow(clippy::result_large_err)]
    pub fn mcp_tool(mut self, config: crate::protocol::models::McpToolConfig) -> Result<Self> {
        self.tools.mcp_tool(config)?;
        Ok(self)
    }

    #[must_use]
    pub fn handlers(mut self, handlers: EventHandlers) -> Self {
        self.handlers = handlers;
        self
    }

    #[must_use]
    pub fn on_text<F, Fut>(mut self, handler: F) -> Self
    where
        F: Fn(String) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<()>> + Send + 'static,
    {
        self.handlers = self.handlers.on_text(handler);
        self
    }

    #[must_use]
    pub fn on_tool_call<F, Fut>(mut self, handler: F) -> Self
    where
        F: Fn(super::ToolCall) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<super::ToolResult>> + Send + 'static,
    {
        self.handlers = self.handlers.on_tool_call(handler);
        self
    }

    #[must_use]
    pub fn on_raw_event<F, Fut>(mut self, handler: F) -> Self
    where
        F: Fn(crate::protocol::server_events::ServerEvent) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<()>> + Send + 'static,
    {
        self.handlers = self.handlers.on_raw_event(handler);
        self
    }

    #[allow(clippy::result_large_err)]
    #[allow(clippy::too_many_lines)]
    fn build(self) -> Result<SessionConfigSnapshot> {
        let api_key = self
            .api_key
            .ok_or_else(|| Error::InvalidClientEvent("api_key required".to_string()))?;
        if self
            .call_id
            .as_ref()
            .is_some_and(|call_id| call_id.trim().is_empty())
        {
            return Err(Error::InvalidClientEvent(
                "call_id must not be empty".to_string(),
            ));
        }
        if self
            .safety_identifier
            .as_ref()
            .is_some_and(|identifier| identifier.trim().is_empty())
        {
            return Err(Error::InvalidClientEvent(
                "safety_identifier must not be empty".to_string(),
            ));
        }
        if self.session_kind == SessionKind::Translation
            && self
                .audio
                .as_ref()
                .and_then(|audio| audio.output.as_ref())
                .and_then(|output| output.language.as_deref())
                .is_some_and(|language| language.trim().is_empty())
        {
            return Err(Error::InvalidClientEvent(
                "translation language must not be empty".to_string(),
            ));
        }
        if self.session_kind == SessionKind::Transcription
            && self
                .audio
                .as_ref()
                .and_then(|audio| audio.input.as_ref())
                .and_then(|input| input.transcription.as_ref())
                .and_then(crate::protocol::models::Nullable::as_ref)
                .and_then(|transcription| transcription.language.as_deref())
                .is_some_and(|language| language.trim().is_empty())
        {
            return Err(Error::InvalidClientEvent(
                "transcription language must not be empty".to_string(),
            ));
        }
        let model = self.model.clone();
        let output_modalities = self.output_modalities.unwrap_or(OutputModalities::Audio);
        let model_name = self.model.unwrap_or_else(|| {
            if self.session_kind == SessionKind::Transcription {
                GPT_REALTIME_WHISPER.to_string()
            } else {
                crate::protocol::models::DEFAULT_MODEL.to_string()
            }
        });

        let mut session = SessionConfig::new(self.session_kind, model_name, output_modalities);
        session.instructions = self.instructions;
        if !self.include.is_empty() {
            session.include = Some(self.include);
        }
        session.tool_choice = self.tool_choice;
        session.temperature = self.temperature;
        session.max_output_tokens = self.max_output_tokens;
        session.reasoning = self.reasoning;
        if let Some(audio) = self.audio {
            session.audio = Some(audio);
        }
        if session.kind == SessionKind::Transcription {
            let audio = session.audio.get_or_insert_with(AudioConfig::default);
            let input = audio.input.get_or_insert_with(InputAudioConfig::default);
            match &mut input.transcription {
                Some(crate::protocol::models::Nullable::Value(transcription)) => {
                    if transcription.model.is_none() {
                        transcription.model = Some(GPT_REALTIME_WHISPER.to_string());
                    }
                }
                Some(crate::protocol::models::Nullable::Null) => {}
                None => {
                    input.transcription = Some(crate::protocol::models::Nullable::Value(
                        InputAudioTranscription {
                            model: Some(GPT_REALTIME_WHISPER.to_string()),
                            language: None,
                            prompt: None,
                        },
                    ));
                }
            }
        }

        let dispatcher = if let Some(d) = self.dispatcher {
            if session.tools.is_none() {
                let defs = d.try_tool_definitions()?;
                if !defs.is_empty() {
                    session.tools = Some(defs);
                }
            }
            d
        } else {
            if !self.tools.is_empty() {
                session.tools = Some(self.tools.try_as_tools()?);
            }
            Arc::new(self.tools)
        };

        Ok(SessionConfigSnapshot {
            api_key,
            model,
            call_id: self.call_id,
            safety_identifier: self.safety_identifier,
            session,
            handlers: self.handlers,
            dispatcher,
            auto_barge_in: self.auto_barge_in,
            auto_tool_response: self.auto_tool_response,
            send_initial_session_update: self.send_initial_session_update,
        })
    }

    const fn session_include_mut(&mut self) -> &mut Vec<String> {
        &mut self.include
    }

    /// Connect via WebSocket using the configured session.
    ///
    /// # Errors
    /// Returns an error if configuration is incomplete or the connection fails.
    pub async fn connect_ws(self) -> Result<super::Session> {
        self.build()?.connect_ws().await
    }
}

impl Default for RealtimeBuilder {
    fn default() -> Self {
        Self::new()
    }
}

pub struct VoiceSessionBuilder {
    inner: RealtimeBuilder,
}

impl VoiceSessionBuilder {
    #[must_use]
    fn new(mut inner: RealtimeBuilder) -> Self {
        let input = InputAudioConfig {
            format: Some(AudioFormat::pcm_24khz()),
            turn_detection: Some(crate::protocol::models::Nullable::Value(
                TurnDetection::ServerVad {
                    threshold: None,
                    prefix_padding_ms: None,
                    silence_duration_ms: None,
                    idle_timeout_ms: None,
                    create_response: Some(true),
                    interrupt_response: Some(true),
                },
            )),
            transcription: None,
            noise_reduction: None,
        };
        let output = OutputAudioConfig {
            format: Some(AudioFormat::pcm_24khz()),
            voice: None,
            speed: None,
            language: None,
        };
        inner.output_modalities = Some(OutputModalities::Audio);
        inner.audio = Some(AudioConfig {
            input: Some(input),
            output: Some(output),
        });
        inner.auto_barge_in = true;
        Self { inner }
    }

    #[must_use]
    pub fn api_key(mut self, key: impl Into<String>) -> Self {
        self.inner = self.inner.api_key(key);
        self
    }

    #[must_use]
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.inner = self.inner.model(model);
        self
    }

    #[must_use]
    pub fn call_id(mut self, call_id: impl Into<String>) -> Self {
        self.inner = self.inner.call_id(call_id);
        self
    }

    #[must_use]
    pub fn safety_identifier(mut self, safety_identifier: impl Into<String>) -> Self {
        self.inner = self.inner.safety_identifier(safety_identifier);
        self
    }

    #[must_use]
    pub fn voice(mut self, voice: impl Into<String>) -> Self {
        self.inner = self.inner.voice(voice);
        if let Some(audio) = self.inner.audio.as_mut() {
            if let Some(output) = audio.output.as_mut() {
                output.voice = self
                    .inner
                    .voice
                    .clone()
                    .map(crate::protocol::models::Voice::from);
            }
        }
        self
    }

    #[must_use]
    pub fn instructions(mut self, instructions: impl Into<String>) -> Self {
        self.inner = self.inner.instructions(instructions);
        self
    }

    #[must_use]
    pub const fn vad_server_default(self) -> Self {
        let vad = TurnDetection::ServerVad {
            threshold: None,
            prefix_padding_ms: None,
            silence_duration_ms: None,
            idle_timeout_ms: None,
            create_response: Some(true),
            interrupt_response: Some(true),
        };
        self.set_turn_detection(vad)
    }

    #[must_use]
    pub const fn set_turn_detection(mut self, vad: TurnDetection) -> Self {
        if let Some(audio) = self.inner.audio.as_mut() {
            if let Some(input) = audio.input.as_mut() {
                input.turn_detection = Some(crate::protocol::models::Nullable::Value(vad));
            }
        }
        self
    }

    #[must_use]
    pub fn transcription(mut self, model: impl Into<String>) -> Self {
        let transcription = InputAudioTranscription {
            model: Some(model.into()),
            language: None,
            prompt: None,
        };
        if let Some(audio) = self.inner.audio.as_mut() {
            if let Some(input) = audio.input.as_mut() {
                input.transcription = Some(crate::protocol::models::Nullable::Value(transcription));
            }
        }
        self
    }

    #[must_use]
    pub const fn noise_reduction(mut self, noise_reduction: NoiseReduction) -> Self {
        if let Some(audio) = self.inner.audio.as_mut() {
            if let Some(input) = audio.input.as_mut() {
                input.noise_reduction =
                    Some(crate::protocol::models::Nullable::Value(noise_reduction));
            }
        }
        self
    }

    #[must_use]
    pub const fn auto_barge_in(mut self, enabled: bool) -> Self {
        self.inner.auto_barge_in = enabled;
        self
    }

    #[must_use]
    pub const fn auto_tool_response(mut self, enabled: bool) -> Self {
        self.inner.auto_tool_response = enabled;
        self
    }

    #[must_use]
    pub fn reasoning_effort(mut self, effort: ReasoningEffort) -> Self {
        self.inner = self.inner.reasoning_effort(effort);
        self
    }

    #[must_use]
    pub fn manual_sideband_control(mut self) -> Self {
        self.inner = self.inner.manual_sideband_control();
        self
    }

    #[must_use]
    pub fn tool_dispatcher(mut self, dispatcher: Arc<dyn ToolDispatcher>) -> Self {
        self.inner.dispatcher = Some(dispatcher);
        self
    }

    #[must_use]
    pub fn tools(mut self, tools: ToolRegistry) -> Self {
        self.inner = self.inner.tools(tools);
        self
    }

    #[must_use]
    pub fn tool<TArgs, TResp, F, Fut>(mut self, name: &str, handler: F) -> Self
    where
        TArgs: schemars::JsonSchema + serde::de::DeserializeOwned + Send + 'static,
        TResp: serde::Serialize + Send + 'static,
        F: Fn(TArgs) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<TResp>> + Send + 'static,
    {
        self.inner = self.inner.tool(name, handler);
        self
    }

    #[must_use]
    pub fn tool_desc<TArgs, TResp, F, Fut>(
        mut self,
        name: &str,
        description: impl Into<String>,
        handler: F,
    ) -> Self
    where
        TArgs: schemars::JsonSchema + serde::de::DeserializeOwned + Send + 'static,
        TResp: serde::Serialize + Send + 'static,
        F: Fn(TArgs) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<TResp>> + Send + 'static,
    {
        self.inner = self.inner.tool_desc(name, description, handler);
        self
    }

    #[must_use]
    pub fn tool_with_description<TArgs, TResp, F, Fut>(
        mut self,
        name: &str,
        description: impl Into<String>,
        handler: F,
    ) -> Self
    where
        TArgs: schemars::JsonSchema + serde::de::DeserializeOwned + Send + 'static,
        TResp: serde::Serialize + Send + 'static,
        F: Fn(TArgs) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<TResp>> + Send + 'static,
    {
        self.inner = self.inner.tool_with_description(name, description, handler);
        self
    }

    #[must_use]
    pub fn on_text<F, Fut>(mut self, handler: F) -> Self
    where
        F: Fn(String) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<()>> + Send + 'static,
    {
        self.inner = self.inner.on_text(handler);
        self
    }

    #[must_use]
    pub fn on_tool_call<F, Fut>(mut self, handler: F) -> Self
    where
        F: Fn(super::ToolCall) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<super::ToolResult>> + Send + 'static,
    {
        self.inner = self.inner.on_tool_call(handler);
        self
    }

    #[must_use]
    pub fn on_raw_event<F, Fut>(mut self, handler: F) -> Self
    where
        F: Fn(crate::protocol::server_events::ServerEvent) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<()>> + Send + 'static,
    {
        self.inner = self.inner.on_raw_event(handler);
        self
    }

    /// Connect via WebSocket using the configured voice session.
    ///
    /// # Errors
    /// Returns an error if configuration is incomplete or the connection fails.
    pub async fn connect_ws(self) -> Result<super::Session> {
        self.inner.connect_ws().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_sideband_control_builds_call_id_attach_without_initial_update() {
        let snapshot = RealtimeBuilder::new()
            .api_key("test-key")
            .model("gpt-realtime")
            .call_id("call_123")
            .manual_sideband_control()
            .build()
            .expect("builder snapshot");

        assert_eq!(snapshot.call_id.as_deref(), Some("call_123"));
        assert!(matches!(
            snapshot.connection_target(),
            super::super::session::SessionConnectTarget::CallId(call_id) if call_id == "call_123"
        ));
        assert!(!snapshot.send_initial_session_update);
        assert!(!snapshot.auto_barge_in);
        assert!(!snapshot.auto_tool_response);
    }

    #[test]
    fn empty_call_id_is_rejected() {
        let result = RealtimeBuilder::new()
            .api_key("test-key")
            .call_id("   ")
            .build();
        let Err(err) = result else {
            panic!("empty call_id should be rejected");
        };

        assert!(matches!(
            err,
            Error::InvalidClientEvent(message) if message.contains("call_id")
        ));
    }

    #[test]
    fn translation_session_builds_dedicated_target() {
        let snapshot = Realtime::translation_builder()
            .api_key("test-key")
            .translation_language("es")
            .build()
            .expect("translation snapshot");

        assert_eq!(snapshot.session.kind, SessionKind::Translation);
        assert!(matches!(
            snapshot.connection_target(),
            super::super::session::SessionConnectTarget::Translation(model)
                if model == GPT_REALTIME_TRANSLATE
        ));
        assert_eq!(
            snapshot
                .session
                .audio
                .as_ref()
                .and_then(|audio| audio.output.as_ref())
                .and_then(|output| output.language.as_deref()),
            Some("es")
        );
    }

    #[test]
    fn transcription_session_builds_intent_target_and_defaults_whisper() {
        let snapshot = Realtime::transcription_builder()
            .api_key("test-key")
            .transcription_language("en")
            .transcription_prompt("Keywords: systolic")
            .include_transcription_logprobs()
            .build()
            .expect("transcription snapshot");

        assert_eq!(snapshot.session.kind, SessionKind::Transcription);
        assert!(matches!(
            snapshot.connection_target(),
            super::super::session::SessionConnectTarget::Transcription
        ));
        assert_eq!(snapshot.session.model, GPT_REALTIME_WHISPER);
        assert_eq!(
            snapshot
                .session
                .audio
                .as_ref()
                .and_then(|audio| audio.input.as_ref())
                .and_then(|input| input.transcription.as_ref())
                .and_then(crate::protocol::models::Nullable::as_ref)
                .and_then(|transcription| transcription.model.as_deref()),
            Some(GPT_REALTIME_WHISPER)
        );
        assert_eq!(
            snapshot
                .session
                .audio
                .as_ref()
                .and_then(|audio| audio.input.as_ref())
                .and_then(|input| input.transcription.as_ref())
                .and_then(crate::protocol::models::Nullable::as_ref)
                .and_then(|transcription| transcription.language.as_deref()),
            Some("en")
        );
        assert_eq!(
            snapshot.session.include.as_deref(),
            Some(&["item.input_audio_transcription.logprobs".to_string()][..])
        );
    }

    #[test]
    fn transcription_model_preserves_existing_prompt_and_language() {
        let snapshot = Realtime::transcription_builder()
            .api_key("test-key")
            .transcription_language("en")
            .transcription_prompt("Keywords: systolic")
            .transcription_model("gpt-4o-transcribe")
            .build()
            .expect("transcription snapshot");

        let transcription = snapshot
            .session
            .audio
            .as_ref()
            .and_then(|audio| audio.input.as_ref())
            .and_then(|input| input.transcription.as_ref())
            .and_then(crate::protocol::models::Nullable::as_ref)
            .expect("transcription config");
        assert_eq!(transcription.model.as_deref(), Some("gpt-4o-transcribe"));
        assert_eq!(transcription.language.as_deref(), Some("en"));
        assert_eq!(transcription.prompt.as_deref(), Some("Keywords: systolic"));
    }

    #[test]
    fn reasoning_effort_is_copied_to_initial_session() {
        let snapshot = RealtimeBuilder::new()
            .api_key("test-key")
            .reasoning_effort(ReasoningEffort::Low)
            .build()
            .expect("snapshot");

        assert_eq!(
            snapshot.session.reasoning.as_ref().and_then(|r| r.effort),
            Some(ReasoningEffort::Low)
        );
    }

    #[test]
    fn safety_identifier_is_validated() {
        let snapshot = RealtimeBuilder::new()
            .api_key("test-key")
            .safety_identifier("hashed-user")
            .build()
            .expect("snapshot");
        assert_eq!(snapshot.safety_identifier.as_deref(), Some("hashed-user"));

        let result = RealtimeBuilder::new()
            .api_key("test-key")
            .safety_identifier(" ")
            .build();
        assert!(matches!(
            result,
            Err(Error::InvalidClientEvent(message)) if message.contains("safety_identifier")
        ));
    }

    #[test]
    fn empty_translation_language_is_rejected() {
        let result = Realtime::translation_builder()
            .api_key("test-key")
            .translation_language(" ")
            .build();

        assert!(matches!(
            result,
            Err(Error::InvalidClientEvent(message)) if message.contains("translation language")
        ));
    }
}
