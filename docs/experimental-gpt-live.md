# Experimental GPT Live protocol

The `experimental-gpt-live` Cargo feature exposes mechanical support for a
private, pre-release voice protocol. The feature is unstable and deliberately
isolated from the crate's GA Realtime API.

## Enable the feature

```toml
[dependencies]
oai-rt-rs = { version = "0.4.1", features = ["experimental-gpt-live"] }
```

The public surface is under `oai_rt_rs::experimental::gpt_live`.

## Supported boundary

The crate owns protocol mechanics:

- bounded and redaction-safe event encoding and decoding
- WebRTC call creation and SDP answer handling
- authenticated sideband WebSocket attachment
- typed session, transcript, turn, and client-delegation events
- forward-compatible retention of unknown fields and events
- client context append and its acknowledgement
- opaque `session.usage.updated` observations

The caller owns semantic and application authority:

- selecting the model and voice
- resolving OAuth credentials and sideband headers
- assigning durable application identity
- deciding when a final transcript is eligible for execution
- executing delegated work, including tools and effects
- correlating provider observations to application-owned interaction identity
- deciding whether a late result may still be spoken

The transport never logs bearer tokens, SDP, transcripts, delegation content,
function arguments, or function output. Debug representations redact those
values, and codec limits reject oversized payloads.

## Client-managed delegation

Use client delegation when work must execute under application authority:

```rust
use oai_rt_rs::experimental::gpt_live::{
    CallSession, ClientDelegation, Delegation, ExtraFields, SessionAudio,
    SessionAudioOutput,
};

let session = CallSession {
    model: "gpt-live-1-codex".to_owned(),
    audio: SessionAudio {
        output: SessionAudioOutput {
            voice: "marin".to_owned(),
            extra: ExtraFields::new(),
        },
        extra: ExtraFields::new(),
    },
    delegation: Some(Delegation::Client(ClientDelegation::default())),
    instructions: None,
    extra: ExtraFields::new(),
};
```

The application supplies this session with its SDP offer to
`GptLiveTransport::create_call`, then attaches the returned opaque provider call
ID with `GptLiveTransport::connect_sideband`.

When the provider emits `delegation.created`, the event carries the provider's
delegation item ID and turn references. Treat those values as observations,
not as application authority. An application should bind them to its own stable
interaction identity, admit execution only after its canonical final-transcript
boundary, and run the executor under ordinary application policy.

Return the executor's result with `ClientEvent::DelegationContextAppend`, using
the exact `delegation_item_id` from the provider event. The provider acknowledges
the append with `delegation.context.appended` and can continue the voice turn.
The crate does not autonomously invoke an executor or retry effects.

The Meerkat integration for this release uses this client-managed mode. The
voice channel remains conversational and tool-less, while a separate Meerkat
executor owns durable work and any permitted effects.

## Responses delegation status

The feature includes wire types for:

- `delegation.type = "responses"`
- nested Responses model and function-tool configuration
- `delegation.function_call_output.create`

These types preserve a captured and independently demonstrated configuration,
but they do not constitute a qualified end-to-end implementation. Direct raw
probes established that the session configuration can start, then twice failed
inside the provider while resolving its internal Responses backend. No raw
function-call event, call identifier, argument stream, output acknowledgement,
automatic continuation, cancellation, or settlement event was observed.

Accordingly:

- do not invent or depend on an inbound Responses function-call event schema
- do not treat the outbound function result type as proof of a complete bridge
- do not enable a production Responses bridge from synthetic fixtures or
  third-party normalization alone
- treat `ResponsesConfig::instructions` as syntactically accepted but
  behaviorally unproven

Applications that require their own executor should use client-managed
delegation instead.

## Usage telemetry

`ServerEvent::SessionUsageUpdated` preserves the event discriminant and unknown
fields for diagnostics. Its payload is intentionally opaque and
non-authoritative. Do not use it to settle billing, determine execution
completion, or mutate durable session state.

## Stability

All types in this feature may change between patch releases while the provider
protocol is private and pre-release. Keep the feature behind an application
experiment flag and fail closed when required protocol evidence is absent.
