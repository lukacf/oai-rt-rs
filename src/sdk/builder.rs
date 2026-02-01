use crate::protocol::models::{
    AudioConfig, AudioFormat, InputAudioConfig, InputAudioTranscription, MaxTokens, NoiseReduction,
    OutputAudioConfig, OutputModalities, SessionConfig, SessionKind, Temperature, ToolChoice,
    TurnDetection,
};
use crate::{Error, Result};

use super::{EventHandlers, ToolRegistry};
use super::session::SessionConfigSnapshot;

pub struct Realtime;

impl Realtime {
    #[must_use]
    pub fn builder() -> RealtimeBuilder {
        RealtimeBuilder::new()
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
    voice: Option<String>,
    output_modalities: Option<OutputModalities>,
    instructions: Option<String>,
    tool_choice: Option<ToolChoice>,
    temperature: Option<Temperature>,
    max_output_tokens: Option<MaxTokens>,
    audio: Option<AudioConfig>,
    auto_barge_in: bool,
    auto_tool_response: bool,
    handlers: EventHandlers,
    tools: ToolRegistry,
}

impl RealtimeBuilder {
    #[must_use]
    pub fn new() -> Self {
        Self {
            api_key: None,
            model: None,
            voice: None,
            output_modalities: None,
            instructions: None,
            tool_choice: None,
            temperature: None,
            max_output_tokens: None,
            audio: None,
            auto_barge_in: false,
            auto_tool_response: true,
            handlers: EventHandlers::new(),
            tools: ToolRegistry::new(),
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
    pub fn voice(mut self, voice: impl Into<String>) -> Self {
        self.voice = Some(voice.into());
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
    fn build(self) -> Result<SessionConfigSnapshot> {
        let api_key = self.api_key.ok_or_else(|| Error::InvalidClientEvent("api_key required".to_string()))?;
        let model = self.model.clone();
        let output_modalities = self.output_modalities.unwrap_or(OutputModalities::Audio);
        let model_name = self.model.unwrap_or_else(|| crate::protocol::models::DEFAULT_MODEL.to_string());

        let mut session = SessionConfig::new(SessionKind::Realtime, model_name, output_modalities);
        if let Some(voice) = self.voice {
            session.voice = Some(crate::protocol::models::Voice::from(voice));
        }
        session.instructions = self.instructions;
        session.tool_choice = self.tool_choice;
        session.temperature = self.temperature;
        session.max_output_tokens = self.max_output_tokens;
        if let Some(audio) = self.audio {
            session.audio = Some(audio);
        }
        if !self.tools.is_empty() {
            session.tools = Some(self.tools.try_as_tools()?);
        }

        Ok(SessionConfigSnapshot {
            api_key,
            model,
            session,
            handlers: self.handlers,
            tools: self.tools,
            auto_barge_in: self.auto_barge_in,
            auto_tool_response: self.auto_tool_response,
        })
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
            turn_detection: Some(crate::protocol::models::Nullable::Value(TurnDetection::ServerVad {
                threshold: None,
                prefix_padding_ms: None,
                silence_duration_ms: None,
                idle_timeout_ms: None,
                create_response: Some(true),
                interrupt_response: Some(true),
            })),
            transcription: None,
            noise_reduction: None,
        };
        let output = OutputAudioConfig {
            format: Some(AudioFormat::pcm_24khz()),
            voice: None,
            speed: None,
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
    pub fn voice(mut self, voice: impl Into<String>) -> Self {
        self.inner = self.inner.voice(voice);
        if let Some(audio) = self.inner.audio.as_mut() {
            if let Some(output) = audio.output.as_mut() {
                output.voice = self.inner.voice.clone().map(crate::protocol::models::Voice::from);
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
    pub fn vad_server_default(self) -> Self {
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
    pub fn set_turn_detection(mut self, vad: TurnDetection) -> Self {
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
    pub fn noise_reduction(mut self, noise_reduction: NoiseReduction) -> Self {
        if let Some(audio) = self.inner.audio.as_mut() {
            if let Some(input) = audio.input.as_mut() {
                input.noise_reduction = Some(crate::protocol::models::Nullable::Value(noise_reduction));
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
