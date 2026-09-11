use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::de::{MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};
use std::{
    fmt,
    io::{self, Write},
};

use super::{
    AudioFormat, ClientConfig, ClientEvent, Command, DelegationConfig, DelegationUpdate, Error,
    EventPermissions, Field, InitialRole, InitialTextType, NamedToolChoice, ResponsesOptions,
    ResponsesUpdate, Result, ServerFrame, SessionConfig, ToolChoice, Voice,
};

/// Local safety limit, not a claim about the provider's maximum event size.
pub const DEFAULT_MAX_EVENT_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug)]
pub struct Codec {
    pub max_event_bytes: usize,
}

impl Default for Codec {
    fn default() -> Self {
        Self {
            max_event_bytes: DEFAULT_MAX_EVENT_BYTES,
        }
    }
}

impl Codec {
    /// Encode the fork-specific first `session.start` message.
    ///
    /// # Errors
    /// Rejects invalid overrides, correlation IDs, or an oversized frame.
    pub fn encode_fork_start(&self, event: &super::ForkStartEvent) -> Result<String> {
        let text = self.encode_bounded(event)?;
        event.validate()?;
        Ok(text)
    }

    /// # Errors
    /// Rejects missing/extra fields, duplicate keys, and invalid fork startup values.
    pub fn decode_fork_start(&self, text: &str) -> Result<super::ForkStartEvent> {
        self.check_size(text)?;
        let event: super::ForkStartEvent = decode_request(text)?;
        event.validate()?;
        Ok(event)
    }

    /// Encode a validated client command without logging its contents.
    ///
    /// # Errors
    /// Rejects invalid configuration, audio, or a frame exceeding the local limit.
    pub fn encode(&self, event: &ClientEvent) -> Result<String> {
        let text = self.encode_bounded(event)?;
        event.validate()?;
        Ok(text)
    }

    fn encode_bounded(self, value: &impl Serialize) -> Result<String> {
        let mut output = BoundedJson::new(self.max_event_bytes);
        let result = serde_json::to_writer(&mut output, value);
        if output.exceeded {
            return Err(Error::Invalid("event exceeds configured byte limit".into()));
        }
        result?;
        String::from_utf8(output.bytes)
            .map_err(|_| Error::Invalid("JSON serializer produced invalid UTF-8".into()))
    }

    /// Strictly decode outgoing JSON; unknown fields and duplicate keys are errors.
    ///
    /// # Errors
    /// Rejects invalid JSON, extra fields, unsupported commands, and invalid values.
    pub fn decode_client(&self, text: &str) -> Result<ClientEvent> {
        self.check_size(text)?;
        let event: ClientEvent = serde_json::from_str(text)?;
        event.validate()?;
        Ok(event)
    }

    /// Decode a webhook body after application-owned signature verification.
    /// This method validates JSON only; it does not authenticate deliveries.
    ///
    /// # Errors
    /// Rejects oversized/duplicate-key JSON or malformed known incoming-call events.
    pub fn decode_webhook(&self, text: &str) -> Result<super::IncomingWebhook> {
        self.check_size(text)?;
        let raw = serde_json::from_str::<UniqueValue>(text)?.0;
        Ok(serde_json::from_value(raw)?)
    }

    /// Decode inbound JSON, retaining unknown events and fields without treating a
    /// malformed known event as a future event.
    ///
    /// # Errors
    /// Rejects malformed known events, duplicate keys, or oversized frames.
    pub fn decode_server(&self, text: &str) -> Result<ServerFrame> {
        self.check_size(text)?;
        let raw = serde_json::from_str::<UniqueValue>(text)?.0;
        serde_json::from_value(raw.clone()).map_err(|source| Error::MalformedEvent { raw, source })
    }

    fn check_size(self, text: &str) -> Result<()> {
        if text.len() > self.max_event_bytes {
            Err(Error::Invalid("event exceeds configured byte limit".into()))
        } else {
            Ok(())
        }
    }
}

struct BoundedJson {
    bytes: Vec<u8>,
    limit: usize,
    exceeded: bool,
}

impl BoundedJson {
    const fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
            exceeded: false,
        }
    }
}

impl Write for BoundedJson {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            self.exceeded = true;
            return Err(io::Error::other("event exceeds configured byte limit"));
        }
        let required = self.bytes.len() + bytes.len();
        if required > self.bytes.capacity() {
            let capacity = required
                .max(self.bytes.capacity().saturating_mul(2))
                .min(self.limit);
            self.bytes
                .try_reserve_exact(capacity - self.bytes.len())
                .map_err(|_| io::Error::other("cannot allocate bounded event buffer"))?;
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'de> Deserialize<'de> for ClientEvent {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Wire {
            #[serde(default)]
            event_id: Field<String>,
            #[serde(flatten)]
            command: Command,
        }
        let raw = UniqueValue::deserialize(deserializer)?.0;
        let wire: Wire = serde_json::from_value(raw.clone()).map_err(serde::de::Error::custom)?;
        let event = Self {
            event_id: wire.event_id,
            command: wire.command,
        };
        let canonical = serde_json::to_value(&event).map_err(serde::de::Error::custom)?;
        reject_unknown_fields(&raw, &canonical).map_err(serde::de::Error::custom)?;
        Ok(event)
    }
}

/// Decode typed HTTP/config inputs without accepting silently discarded fields.
///
/// # Errors
/// Returns an error for duplicate/unknown fields or an invalid schema.
pub fn decode_request<T: serde::de::DeserializeOwned + Serialize>(text: &str) -> Result<T> {
    let raw = serde_json::from_str::<UniqueValue>(text)?.0;
    let request: T = serde_json::from_value(raw.clone())?;
    reject_unknown_fields(&raw, &serde_json::to_value(&request)?)?;
    Ok(request)
}

fn reject_unknown_fields(raw: &Value, canonical: &Value) -> Result<()> {
    match (raw, canonical) {
        (Value::Object(raw), Value::Object(canonical)) => {
            for (key, value) in raw {
                let Some(expected) = canonical.get(key) else {
                    return Err(Error::Invalid("unknown outgoing field".into()));
                };
                reject_unknown_fields(value, expected)?;
            }
        }
        (Value::Array(raw), Value::Array(canonical)) => {
            for (value, expected) in raw.iter().zip(canonical) {
                reject_unknown_fields(value, expected)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Decode raw base64 audio. No resampling or media-container conversion occurs.
///
/// # Errors
/// Rejects invalid base64, empty audio, PCM half-samples, and WAV containers.
pub fn decode_audio(audio: &str, format: AudioFormat) -> Result<Vec<u8>> {
    format.validate()?;
    let bytes = STANDARD
        .decode(audio)
        .map_err(|_| Error::Invalid("audio must be standard base64".into()))?;
    validate_audio_bytes(&bytes, format)?;
    Ok(bytes)
}

/// Validate raw audio bytes for the selected immutable format.
///
/// # Errors
/// Rejects empty audio, incomplete PCM samples, or WAV headers.
pub fn validate_audio_bytes(bytes: &[u8], format: AudioFormat) -> Result<()> {
    format.validate()?;
    if bytes.is_empty() {
        return Err(Error::Invalid("audio must not be empty".into()));
    }
    if matches!(format, AudioFormat::Pcm { .. }) && bytes.len() % 2 != 0 {
        return Err(Error::Invalid(
            "PCM audio must contain complete 16-bit samples".into(),
        ));
    }
    if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WAVE") {
        return Err(Error::Invalid("send raw audio, not a WAV container".into()));
    }
    Ok(())
}

impl AudioFormat {
    /// # Errors
    /// Rejects rates outside the released Live formats.
    pub fn validate(self) -> Result<()> {
        match self {
            Self::Pcm {
                rate: 16_000 | 24_000,
            }
            | Self::Pcmu { rate: 8_000 }
            | Self::Pcma { rate: 8_000 } => Ok(()),
            _ => Err(Error::Invalid("unsupported Live audio rate".into())),
        }
    }

    #[must_use]
    pub const fn sample_rate(self) -> u32 {
        match self {
            Self::Pcm { rate } | Self::Pcmu { rate } | Self::Pcma { rate } => rate,
        }
    }

    #[must_use]
    pub const fn bytes_per_sample(self) -> usize {
        if matches!(self, Self::Pcm { .. }) {
            2
        } else {
            1
        }
    }
}

impl SessionConfig {
    /// Validate constraints that do not require the provider tokenizer.
    ///
    /// # Errors
    /// Rejects invalid format, history shape, selectors, and backend settings.
    pub fn validate(&self) -> Result<()> {
        if self.model.is_empty() {
            return Err(Error::Invalid("model is required".into()));
        }
        if let Some(audio) = &self.audio {
            if let Some(format) = audio.format {
                format.validate()?;
            }
            if let Some(Voice::Custom { id }) = audio.output.as_ref().and_then(|o| o.voice.as_ref())
            {
                length(id, 1, 128)?;
            }
        }
        if let Some(client) = &self.client {
            client.validate()?;
        }
        if let Some(input) = &self.input {
            if input.len() > 128 {
                return Err(Error::Invalid(
                    "startup history exceeds 128 messages".into(),
                ));
            }
            for item in input {
                if item.content.len() != 1 {
                    return Err(Error::Invalid(
                        "startup messages require exactly one text part".into(),
                    ));
                }
                let valid = match item.role {
                    InitialRole::Developer | InitialRole::User => {
                        matches!(
                            item.content[0].text_type,
                            None | Some(InitialTextType::InputText)
                        )
                    }
                    InitialRole::Assistant => {
                        matches!(
                            item.content[0].text_type,
                            None | Some(InitialTextType::Text | InitialTextType::OutputText)
                        )
                    }
                };
                if !valid {
                    return Err(Error::Invalid(
                        "startup text part does not match its role".into(),
                    ));
                }
            }
        }
        if let Field::Value(DelegationConfig::Responses { responses }) = &self.delegation {
            if responses.model.is_empty() {
                return Err(Error::Invalid("Responses backend model is required".into()));
            }
            responses.options.validate()?;
        }
        Ok(())
    }

    #[must_use]
    pub fn audio_format(&self) -> AudioFormat {
        self.audio
            .as_ref()
            .and_then(|audio| audio.format)
            .unwrap_or_default()
    }
}

impl ClientConfig {
    /// # Errors
    /// Rejects invalid event selectors and missing/irrelevant nested selectors.
    pub fn validate(&self) -> Result<()> {
        if let Some(EventPermissions::Selected(names)) = &self.data_channel.allowed_client_events {
            if names.len() > 256 {
                return Err(Error::Invalid(
                    "client event permissions exceed 256 entries".into(),
                ));
            }
            for name in names {
                length(name, 1, 256)?;
                if !valid_client_event_name(name) {
                    return Err(Error::Invalid(
                        "invalid client event permission name".into(),
                    ));
                }
            }
        }
        if let Some(EventPermissions::Selected(selectors)) =
            &self.data_channel.allowed_server_events
        {
            if selectors.len() > 256 {
                return Err(Error::Invalid(
                    "server event permissions exceed 256 entries".into(),
                ));
            }
            for selector in selectors {
                length(&selector.event_type, 1, 256)?;
                if selector.event_type == "response.event" && selector.response_event.is_none() {
                    return Err(Error::Invalid(
                        "response.event requires a nested response_event selector".into(),
                    ));
                }
                if let Some(nested) = &selector.response_event {
                    length(nested, 1, 256)?;
                    if selector.event_type != "response.event" {
                        return Err(Error::Invalid(
                            "response_event selector requires response.event".into(),
                        ));
                    }
                }
            }
        }
        Ok(())
    }
}

fn valid_client_event_name(name: &str) -> bool {
    if matches!(name, "error" | "info") {
        return true;
    }
    let mut parts = name.split('.');
    let first = parts.next().unwrap_or("");
    let valid_part = |part: &str| {
        !part.is_empty()
            && part.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
            })
    };
    first.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
        && valid_part(first)
        && name.contains('.')
        && parts.all(valid_part)
}

impl ResponsesUpdate {
    /// # Errors
    /// Rejects an empty model or invalid supplied backend options.
    pub fn validate(&self) -> Result<()> {
        if self.model.as_deref() == Some("") {
            return Err(Error::Invalid("Responses backend model is required".into()));
        }
        self.options.validate()
    }
}

impl ResponsesOptions {
    /// # Errors
    /// Rejects invalid numeric limits and named tool selectors.
    pub fn validate(&self) -> Result<()> {
        if let Field::Value(tokens) = self.max_output_tokens {
            if tokens < 16 {
                return Err(Error::Invalid(
                    "max_output_tokens must be an integer of at least 16".into(),
                ));
            }
        }
        if let Some(ToolChoice::Named(choice)) = &self.tool_choice {
            match choice {
                NamedToolChoice::Function { name } => tool_choice_name(name)?,
                NamedToolChoice::Mcp { name, server_label } => {
                    tool_choice_name(name)?;
                    tool_choice_name(server_label)?;
                }
            }
        }
        Ok(())
    }
}

impl ClientEvent {
    /// Validate the wire shape only. Session/transport rules are enforced by the
    /// connection, and tokenizer-dependent limits by the provider.
    ///
    /// # Errors
    /// Rejects invalid correlation IDs, configuration, context IDs, or audio.
    pub fn validate(&self) -> Result<()> {
        if let Field::Value(id) = &self.event_id {
            length(id, 0, 512)?;
        }
        match &self.command {
            Command::Start { session } => session.validate(),
            Command::Update { session } => {
                if let Field::Value(DelegationUpdate::Responses {
                    responses: Some(responses),
                }) = &session.delegation
                {
                    responses.validate()?;
                }
                Ok(())
            }
            Command::InstructionsAppend { delegation_id, .. }
            | Command::ThinkingAppend { delegation_id, .. }
            | Command::CommentaryAppend { delegation_id, .. } => {
                if let Some(id) = &delegation_id.0 {
                    length(id, 1, usize::MAX)?;
                }
                Ok(())
            }
            Command::ResponseItemCreate { item } => item.validate().map_err(Error::Invalid),
            Command::InputAudioAppend { audio } => {
                if STANDARD
                    .decode(audio)
                    .map_or(true, |bytes| bytes.is_empty())
                {
                    return Err(Error::Invalid(
                        "audio must be nonempty standard base64".into(),
                    ));
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

fn tool_choice_name(value: &str) -> Result<()> {
    length(value, 1, 64)?;
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(Error::Invalid(
            "named tool choice must use ASCII letters, digits, underscores or hyphens".into(),
        ));
    }
    Ok(())
}

fn length(value: &str, min: usize, max: usize) -> Result<()> {
    if (min..=max).contains(&value.chars().count()) {
        Ok(())
    } else {
        Err(Error::Invalid("string length outside schema bounds".into()))
    }
}

struct UniqueValue(Value);

impl<'de> Deserialize<'de> for UniqueValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct UniqueVisitor;
        impl<'de> Visitor<'de> for UniqueVisitor {
            type Value = UniqueValue;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("JSON with unique object keys")
            }
            fn visit_map<M: MapAccess<'de>>(
                self,
                mut map: M,
            ) -> std::result::Result<Self::Value, M::Error> {
                let mut values = Map::new();
                while let Some((key, value)) = map.next_entry::<String, UniqueValue>()? {
                    if values.insert(key, value.0).is_some() {
                        return Err(serde::de::Error::custom("duplicate JSON key"));
                    }
                }
                Ok(UniqueValue(Value::Object(values)))
            }
            fn visit_seq<S: SeqAccess<'de>>(
                self,
                mut seq: S,
            ) -> std::result::Result<Self::Value, S::Error> {
                let mut values = Vec::new();
                while let Some(value) = seq.next_element::<UniqueValue>()? {
                    values.push(value.0);
                }
                Ok(UniqueValue(Value::Array(values)))
            }
            fn visit_bool<E: serde::de::Error>(
                self,
                v: bool,
            ) -> std::result::Result<Self::Value, E> {
                Ok(UniqueValue(Value::Bool(v)))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> std::result::Result<Self::Value, E> {
                Ok(UniqueValue(v.into()))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> std::result::Result<Self::Value, E> {
                Ok(UniqueValue(v.into()))
            }
            fn visit_f64<E: serde::de::Error>(self, v: f64) -> std::result::Result<Self::Value, E> {
                Ok(UniqueValue(v.into()))
            }
            fn visit_str<E: serde::de::Error>(
                self,
                v: &str,
            ) -> std::result::Result<Self::Value, E> {
                Ok(UniqueValue(v.into()))
            }
            fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Self::Value, E> {
                Ok(UniqueValue(Value::Null))
            }
        }
        deserializer.deserialize_any(UniqueVisitor)
    }
}

#[cfg(test)]
mod bounded_encoding_tests {
    use super::*;

    #[test]
    fn serialized_buffer_allocation_never_exceeds_the_wire_budget() {
        for content in ["x".repeat(128 * 1024), "\n".repeat(128 * 1024)] {
            let event = ClientEvent::new(Command::ThinkingAppend {
                content,
                delegation_id: super::super::Nullable(None),
            });
            let mut output = BoundedJson::new(512);
            assert!(serde_json::to_writer(&mut output, &event).is_err());
            assert!(output.exceeded);
            assert!(output.bytes.len() <= 512);
            assert!(
                output.bytes.capacity() <= 512,
                "serializer must not reserve the full oversized payload"
            );
            assert!(matches!(
                Codec {
                    max_event_bytes: 512
                }
                .encode(&event),
                Err(Error::Invalid(_))
            ));
        }
    }

    #[test]
    fn byte_limit_includes_escaping_and_is_inclusive() {
        let event = ClientEvent::new(Command::ThinkingAppend {
            content: "é\n\"".into(),
            delegation_id: super::super::Nullable(None),
        });
        let expected = serde_json::to_string(&event).unwrap();
        assert_eq!(
            Codec {
                max_event_bytes: expected.len()
            }
            .encode(&event)
            .unwrap(),
            expected
        );
        for limit in [0, expected.len() - 1] {
            assert!(matches!(
                Codec {
                    max_event_bytes: limit
                }
                .encode(&event),
                Err(Error::Invalid(_))
            ));
        }
        let fork = super::super::ForkStartEvent::new(super::super::ForkSessionConfig::default());
        let expected = serde_json::to_string(&fork).unwrap();
        assert_eq!(
            Codec {
                max_event_bytes: expected.len()
            }
            .encode_fork_start(&fork)
            .unwrap(),
            expected
        );
        assert!(
            Codec {
                max_event_bytes: expected.len() - 1
            }
            .encode_fork_start(&fork)
            .is_err()
        );
    }
}
