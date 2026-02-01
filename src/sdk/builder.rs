use crate::protocol::models::{OutputModalities, SessionConfig, SessionKind};
use crate::transport::ws::ProtocolVersion;
use crate::{Error, Result};

use super::{EventHandlers, ToolRegistry};
use super::session::SessionConfigSnapshot;

pub struct Realtime;

impl Realtime {
    #[must_use]
    pub fn builder() -> RealtimeBuilder {
        RealtimeBuilder::new()
    }
}

pub struct RealtimeBuilder {
    api_key: Option<String>,
    model: Option<String>,
    voice: Option<String>,
    output_modalities: Option<OutputModalities>,
    handlers: EventHandlers,
    tools: ToolRegistry,
    protocol: ProtocolVersion,
}

impl RealtimeBuilder {
    #[must_use]
    pub fn new() -> Self {
        Self {
            api_key: None,
            model: None,
            voice: None,
            output_modalities: None,
            handlers: EventHandlers::new(),
            tools: ToolRegistry::new(),
            protocol: ProtocolVersion::Ga,
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
    pub fn handlers(mut self, handlers: EventHandlers) -> Self {
        self.handlers = handlers;
        self
    }

    #[must_use]
    pub const fn protocol(mut self, protocol: ProtocolVersion) -> Self {
        self.protocol = protocol;
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

        Ok(SessionConfigSnapshot {
            api_key,
            model,
            session,
            handlers: self.handlers,
            tools: self.tools,
            protocol: self.protocol,
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
