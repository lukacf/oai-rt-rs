use crate::protocol::models::{
    MaxTokens, OutputModalities, SessionConfig, SessionKind, Temperature, ToolChoice,
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
        if !self.tools.is_empty() {
            session.tools = Some(self.tools.try_as_tools()?);
        }

        Ok(SessionConfigSnapshot {
            api_key,
            model,
            session,
            handlers: self.handlers,
            tools: self.tools,
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
