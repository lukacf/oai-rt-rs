# Rust OpenAI Realtime and Live SDK

Use `oai_rt_rs::live::LiveClient` for full-duplex GPT-Live-1.1 conversations
with delegated backend work. Use `oai_rt_rs::Realtime` for turn-based voice
agents, streaming translation, or transcription. [Public Live guide](docs/live.md)
and [OpenAI Realtime guide](https://developers.openai.com/api/docs/guides/realtime).

[![Crates.io](https://img.shields.io/crates/v/oai-rt-rs.svg)](https://crates.io/crates/oai-rt-rs)
[![Documentation](https://docs.rs/oai-rt-rs/badge.svg)](https://docs.rs/oai-rt-rs)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

## Features

- GA-aligned Realtime API protocol models (WebSocket + REST).
- GA-only behavior: beta headers/events are not supported (e.g., `response.output_item.created`).
- Voice-first SDK with full-duplex audio streaming, VAD, and barge-in helpers.
- GPT Realtime 2.1 support with configurable reasoning effort.
- First-class streaming translation and transcription session helpers.
- Strongly typed `ClientEvent` and `ServerEvent` enums.
- WebRTC SDP signaling, SIP control endpoints, and call hangup (low-level REST).
- Sideband WebSocket attach for existing calls via `call_id`.
- Optional `OpenAI-Safety-Identifier` headers for WebSocket, REST session creation, and WebRTC calls.
- Async interface using `tokio` and `tokio-tungstenite`.
- Client-side validation for GA constraints (PCM 24kHz, output modalities, 15MB audio chunks).
- Feature-gated mechanical support for the private, pre-release GPT Live protocol.
- Public GPT-Live types, strict codecs, bounded full-duplex WebSockets, and explicit final usage.

## Public GPT-Live

```toml
[dependencies]
oai-rt-rs = "0.6.0"
```

In 0.6.0, `live::SessionConfig::default()` selects `gpt-live-1.1` for new
WebSocket, WebRTC and inbound SIP sessions. Forks inherit the source session's
model. Existing applications can explicitly select `live::GPT_LIVE_1`.
OpenAI supports both Live models. [GPT-Live model release](https://developers.openai.com/api/docs/changelog)

```rust,no_run
use oai_rt_rs::live::{Field, LiveClient, LiveConnection, Result, SessionConfig};

async fn open_order_session(key: &str) -> Result<LiveConnection> {
    let client = LiveClient::new(key)?;
    client.connect(SessionConfig {
        instructions: Field::Value(
            "Delegate to the backend when the caller asks for order status.".into(),
        ),
        ..SessionConfig::default()
    }).await
}
```

The [public Live guide](docs/live.md) covers WebRTC standby, client and
Responses delegation, final usage, and runnable voice probes.

## Migrating older Realtime voice models

Realtime voice sessions now default to `gpt-realtime-2.1`. When an explicit
model ID has a scheduled retirement, the crate sends the requested ID and
emits a `tracing` warning with its shutdown date and replacement.
[OpenAI deprecation schedule](https://developers.openai.com/api/docs/deprecations)

| Model IDs in existing configuration | Shutdown | Replacement |
| --- | --- | --- |
| `gpt-realtime`, `gpt-realtime-2025-08-28`, `gpt-4o-realtime` | 2027-01-20 | `GPT_REALTIME_2_1` |
| `gpt-realtime-mini`, `gpt-realtime-mini-2025-12-15`, `gpt-4o-mini-realtime` | 2027-01-20 | `GPT_REALTIME_2_1_MINI` |
| `gpt-4o-mini-transcribe-2025-03-20` | 2027-01-20 | Move directly to `GPT_LIVE_TRANSCRIBE` for streaming or `GPT_TRANSCRIBE` for committed turns. The intermediate December 2025 snapshot also has a shutdown date. |
| `whisper-1`, `gpt-4o-transcribe`, `gpt-4o-mini-transcribe`, `gpt-4o-mini-transcribe-2025-12-15`, `gpt-4o-transcribe-diarize` | 2027-02-26 | `GPT_LIVE_TRANSCRIBE` for streaming or `GPT_TRANSCRIBE` for committed turns over WebSocket. Check speaker-label requirements before moving a diarization workflow. |

For browser Realtime sessions, replace `RealtimeRestAdapter::create_session`
with `create_client_secret`. For transcription sessions, replace
`create_transcription_session` with `create_transcription_client_secret`.
For a trusted transcription WebSocket, the builder sends the GA
`session.update` event with nested audio configuration. [Realtime transcription guide](https://developers.openai.com/api/docs/guides/realtime-transcription)

### Rust source changes in 0.6

- Add `..Default::default()` to explicit `live::SessionConfig` and
  `InputAudioTranscription` literals to omit the new options. For explicit
  `live::Usage` literals, set the new `active_seconds` and `paused_seconds`
  fields to `None` if you only have the total.
- Handle `Paused` and `Resumed` in exhaustive matches on `live::ServerEvent`.
  The Realtime `ServerEvent` also has `TranscriptionSessionCreated` and
  `TranscriptionSessionUpdated` variants carrying input-only session snapshots.
- On transcription completion, both the protocol event and
  `SdkEvent::InputTranscriptionCompleted` expose optional `languages` and
  `Option<TranscriptionUsage>`. Handle `Tokens(usage)` and
  `Duration { seconds }` independently when recording billing.

The [vendored Realtime guides](docs/openai/) retain historical examples that
use older model IDs. Use the migration table above when adapting them or
consult the [current Realtime guide](https://developers.openai.com/api/docs/guides/realtime).

## Experimental GPT Live protocol

This section describes the **private adapter only**, not the public GPT-Live API.
New public integrations should use [`oai_rt_rs::live`](docs/live.md).

Enable the private GPT Live protocol surface explicitly:

```toml
[dependencies]
oai-rt-rs = { version = "0.6.0", features = ["experimental-gpt-live"] }
```

This feature is intentionally separate from the GA Realtime API. It provides
redaction-safe wire types, bounded codecs, WebRTC call creation, sideband
transport, transcript and turn events, and client-managed delegation context.
It does not select a model, obtain OAuth credentials, assign application
identity, execute delegated work, or decide what provider events mean to an
application.

For an application-owned executor, configure `delegation.type = "client"`.
The provider can emit a typed `delegation.created` event, after which the
application performs the work under its own authority and returns context with
`delegation.context.append`. The current Meerkat integration uses this mode so
that Meerkat, rather than a provider-selected Responses model, owns execution.

The crate also mechanically encodes `delegation.type = "responses"`, its
nested function-tool configuration, and
`delegation.function_call_output.create`. That mode is not qualified for
production use here: direct probes reached session startup but failed inside
the provider before any raw function-call event was observed. Its inbound call
schema, output acknowledgement, continuation, cancellation, and settlement
lifecycle therefore remain unproven. Nested `responses.instructions` has only
been shown to be syntactically accepted, not behaviorally effective.

`session.usage.updated` is exposed as an opaque observation. Its body must not
be used as billing, execution, or session authority.

See [Experimental GPT Live protocol](docs/experimental-gpt-live.md) for the
supported boundary and a client-delegation example.

## Realtime voice quickstart

```rust
use oai_rt_rs::Realtime;

#[tokio::main]
async fn main() -> oai_rt_rs::Result<()> {
    let mut session = Realtime::builder()
        .api_key("your-api-key")
        .model(oai_rt_rs::GPT_REALTIME_2_1)
        .voice_session()
        .voice("marin")
        .vad_server_default()
        .transcription(oai_rt_rs::GPT_REALTIME_WHISPER)
        .reasoning_effort(oai_rt_rs::ReasoningEffort::Low)
        .safety_identifier("hashed-user-id")
        .auto_barge_in(true)
        .connect_ws()
        .await?;

    // Stream voice events (audio deltas + transcripts).
    while let Some(evt) = session.next_voice_event().await? {
        match evt {
            oai_rt_rs::VoiceEvent::AudioDelta { pcm, .. } => {
                // play PCM16 @ 24kHz
                println!("audio bytes: {}", pcm.len());
            }
            oai_rt_rs::VoiceEvent::TranscriptDone { transcript, .. } => {
                println!("assistant: {transcript}");
            }
            _ => {}
        }
    }
    Ok(())
}
```

## Barge-in

```rust
# async fn demo(session: &oai_rt_rs::RealtimeSession) -> oai_rt_rs::Result<()> {
// Manually barge-in (clear output + cancel active response).
session.barge_in().await?;
# Ok(())
# }
```

## Convenience audio/transcript streams

```rust
# async fn demo(mut session: oai_rt_rs::RealtimeSession) -> oai_rt_rs::Result<()> {
if let Some(chunk) = session.next_audio_chunk().await? {
    println!("audio bytes: {}", chunk.pcm.len());
}
if let Some(tx) = session.next_transcript().await? {
    println!("transcript: {}", tx.text);
}
# Ok(())
# }
```

## Response builder (high-level)

```rust
use oai_rt_rs::ResponseBuilder;

# async fn demo(session: &oai_rt_rs::RealtimeSession) -> oai_rt_rs::Result<()> {
ResponseBuilder::new()
    .output_text()
    .instructions("Be concise.")
    .input_text("Summarize this.")
    .send(session)
    .await?;
# Ok(())
# }
```

## Sending microphone audio

```rust
# async fn demo(mut session: oai_rt_rs::RealtimeSession) -> oai_rt_rs::Result<()> {
let pcm_samples: Vec<i16> = vec![0; 2400]; // 100ms @ 24kHz
session.audio().send_pcm16(&pcm_samples).await?;
# Ok(())
# }
```

## Streaming microphone audio

```rust
# async fn demo(session: oai_rt_rs::RealtimeSession) -> oai_rt_rs::Result<()> {
let chunks = vec![vec![0i16; 480], vec![1i16; 480]];
let stream = futures::stream::iter(chunks);
session.stream_audio_pcm16(stream).await?;
# Ok(())
# }
```

## Typed tools (simple)

```rust
use oai_rt_rs::Realtime;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SumArgs {
    pub a: i32,
    pub b: i32,
}

#[derive(Debug, Serialize)]
pub struct SumResp {
    pub sum: i32,
}

# async fn demo() -> oai_rt_rs::Result<()> {
let _session = Realtime::builder()
    .api_key("your-api-key")
    .model(oai_rt_rs::GPT_REALTIME_2_1)
    .voice_session()
    .tool_desc("sum", "Add two integers.", |args: SumArgs| async move {
        Ok(SumResp { sum: args.a + args.b })
    })
    .connect_ws()
    .await?;
# // By default, tool success triggers an automatic response.create.
# // Disable with Realtime::builder().auto_tool_response(false).
# Ok(())
# }
```

## Low-level protocol (full control)

```rust
use oai_rt_rs::RealtimeClient;
use oai_rt_rs::protocol::client_events::ClientEvent;
use oai_rt_rs::protocol::models::{SessionUpdate, SessionUpdateConfig, OutputModalities};

#[tokio::main]
async fn main() -> oai_rt_rs::Result<()> {
    let mut client = RealtimeClient::connect("your-api-key", None, None).await?;

    let session = SessionUpdate {
        config: SessionUpdateConfig {
            output_modalities: Some(OutputModalities::Audio),
            instructions: Some("You are a helpful assistant.".to_string()),
            ..SessionUpdateConfig::default()
        },
    };
    client
        .send(ClientEvent::SessionUpdate { event_id: None, session: Box::new(session) })
        .await?;

    while let Some(event) = client.next_event().await? {
        println!("Received event: {:?}", event);
    }
    Ok(())
}
```

## Realtime translation

Translation sessions use the dedicated `/v1/realtime/translations` WebSocket
endpoint and the `gpt-realtime-translate` model. A browser sends its WebRTC SDP
offer to `POST /v1/realtime/translations/calls` with a translation client
secret. Translation begins when audio arrives. [Realtime translation guide](https://developers.openai.com/api/docs/guides/realtime-translation)

```rust
use oai_rt_rs::{Realtime, SdkEvent};

# async fn demo() -> oai_rt_rs::Result<()> {
let mut session = Realtime::translation_builder()
    .api_key("your-api-key")
    .translation_language("es")
    .safety_identifier("hashed-user-id")
    .connect_ws()
    .await?;

let pcm_samples = vec![0i16; 2400];
session.translation_audio_append_pcm16(&pcm_samples).await?;

while let Some(event) = session.next_event().await? {
    match event {
        SdkEvent::TranslationAudioDelta { delta } => {
            println!("translated audio base64 bytes: {}", delta.len());
        }
        SdkEvent::TranslationOutputTranscriptDelta { delta } => {
            print!("{delta}");
        }
        SdkEvent::TranslationInputTranscriptDelta { delta } => {
            eprint!("{delta}");
        }
        _ => {}
    }
}
# Ok(())
# }
```

## Realtime transcription

The transcription builder connects to `/v1/realtime?intent=transcription`
and sends `session.update` with `session.type = "transcription"` and the model
in `session.audio.input.transcription.model`. Its default is
`gpt-realtime-whisper`. Select `GPT_LIVE_TRANSCRIBE` for streaming captions
and commit turns from the client. Select `GPT_TRANSCRIBE` to begin
transcription after each committed WebSocket turn. [Realtime transcription guide](https://developers.openai.com/api/docs/guides/realtime-transcription)

```rust
use oai_rt_rs::{GPT_LIVE_TRANSCRIBE, InputAudioTranscription, Realtime};

# async fn demo() -> oai_rt_rs::Result<()> {
let mut session = Realtime::transcription_builder()
    .api_key("your-api-key")
    .transcription_config(InputAudioTranscription {
        model: Some(GPT_LIVE_TRANSCRIBE.into()),
        languages: Some(vec!["en".into(), "fr".into()]),
        keywords: Some(vec!["metoprolol".into()]),
        ..Default::default()
    })
    .manual_turn_detection()
    .transcription_prompt("A conversation about blood pressure medication.")
    .connect_ws()
    .await?;

session.audio_in_append_pcm16(&[0i16; 2400]).await?;
session.audio_in_commit().await?;

while let Some(event) = session.next_event().await? {
    if let oai_rt_rs::SdkEvent::InputTranscriptionDelta { delta, .. } = event {
        print!("{delta}");
    }
}
# Ok(())
# }
```

Transcription prompts are best-effort steering for vocabulary, spelling,
punctuation, and light formatting. For enriched transcripts with markers such as
`[laughing]` or pronunciation notes, keep the prompt short and treat the output
as advisory. For stronger instruction following, run an out-of-band Realtime
text response after each committed audio turn with `conversation: "none"` and
`output_modalities: ["text"]`.

## Realtime phases and preambles

`gpt-realtime-2.1` can produce intermediate commentary, including spoken
preambles around tool use, before its final answer. Response output items expose
this as `ResponsePhase::Commentary` or `ResponsePhase::FinalAnswer`, so apps can
show or play short progress updates differently from final user-facing content.

## Runnable examples

The repository includes sample apps that show live transcript deltas as they
arrive:

```bash
OPENAI_API_KEY=... cargo run --example ws_voice_tools_mic
OPENAI_API_KEY=... cargo run --example ws_translate_mic -- es
OPENAI_API_KEY=... cargo run --example webrtc_server
```

- `ws_voice_tools_mic` is a WebSocket voice-to-voice session with a typed `sum`
  tool, live user and assistant transcript deltas, assistant audio playback, and
  SDK auto-barge-in. This example is headset-first: it uses raw device capture
  and playback and does not provide acoustic echo cancellation. For
  speakerphone-style full duplex, prefer the browser WebRTC example or a native
  audio stack with platform AEC.
- `ws_translate_mic` is a WebSocket microphone translation stream. It requires a
  default input device and converts the capture stream to the 24 kHz mono PCM
  expected by the Realtime API while printing source and translated transcript
  deltas live.
- `webrtc_server` serves browser WebRTC mini-apps at `http://127.0.0.1:3000/`.
  It mints ephemeral voice and translation sessions on the server, keeps your
  standard API key out of the browser, and propagates `OPENAI_SAFETY_IDENTIFIER`
  when set. The chat page includes local `sum` tool calling and a server-side
  `web_search` bridge that calls the Responses API hosted web search tool,
  streams a short spoken preamble, and displays clickable citations. Set
  `OPENAI_WEB_SEARCH_MODEL` to override the default search model (`gpt-5.5`).

### Audio echo cancellation

The SDK transports Realtime audio and events, but it does not implement acoustic
echo cancellation (AEC). WebSocket/native examples that capture a microphone and
play assistant audio through speakers can feed the assistant's audio back into
the microphone unless the user wears headphones or the application integrates a
platform AEC stack. Browser WebRTC is the recommended sample path for
speakerphone-grade full-duplex voice because the browser media pipeline can
provide echo cancellation, noise suppression, and automatic gain control.

`session.update` and `response.create` request configs serialize sparsely:
unset `Option` fields are omitted from JSON. Fields modeled as `Nullable<T>`
can still send an intentional `null` by using `Some(Nullable::Null)`, for
example to disable turn detection.

```rust
use oai_rt_rs::protocol::models::{Nullable, SessionUpdate, SessionUpdateConfig};

let session = SessionUpdate {
    config: SessionUpdateConfig {
        turn_detection: Some(Nullable::Null),
        ..SessionUpdateConfig::default()
    },
};
```

## Sideband control

Attach a high-level SDK session to an existing Realtime call with `call_id`.
Manual sideband control disables automatic barge-in handling, automatic tool
responses, and the initial SDK-generated `session.update`.

```rust
use oai_rt_rs::Realtime;

# async fn demo() -> oai_rt_rs::Result<()> {
let session = Realtime::builder()
    .api_key("your-api-key")
    .call_id("call_123")
    .manual_sideband_control()
    .connect_ws()
    .await?;

session.respond().await?;
# Ok(())
# }
```

## REST helpers (WebRTC/SIP)

Use the low-level REST adapter for call control:

```rust
use oai_rt_rs::transport::rest::RealtimeRestAdapter;
use oai_rt_rs::protocol::models::{SessionConfig, SessionKind, OutputModalities};

# async fn demo() -> oai_rt_rs::Result<()> {
let rest = RealtimeRestAdapter::new("your-api-key")?;
let session = SessionConfig::new(
    SessionKind::Realtime,
    oai_rt_rs::GPT_REALTIME_2_1,
    OutputModalities::Audio,
);

// WebRTC (raw SDP) + call_id capture
let resp = rest.post_sdp_offer_raw_with_call_id("v=0...".to_string()).await?;
println!("call_id: {:?}", resp.call_id);

// Hang up
if let Some(call_id) = resp.call_id.as_deref() {
    rest.hangup(call_id).await?;
}
# Ok(())
# }
```

## GA constraints (no beta)

- `output_modalities` must be exactly one of `audio` or `text`.
- `audio/pcm` rate is fixed at 24 kHz.
- `input_audio_buffer.append` chunks must be ≤ 15 MB (base64-decoded).
- Invalid GA inputs are rejected client-side with `Error::InvalidClientEvent`.

## MCP example

```rust
use oai_rt_rs::protocol::models::{
    McpToolConfig, RequireApproval, ApprovalMode, Tool, ToolChoice, ToolChoiceMode, SessionUpdate, SessionUpdateConfig,
};
use oai_rt_rs::protocol::client_events::ClientEvent;
use oai_rt_rs::protocol::models::SessionUpdate;

# async fn demo(mut client: oai_rt_rs::RealtimeClient) -> oai_rt_rs::Result<()> {
let mcp = Tool::Mcp(McpToolConfig {
    server_label: "weather".to_string(),
    server_url: Some("https://mcp.example.com".to_string()),
    require_approval: Some(RequireApproval::Mode(ApprovalMode::Always)),
    ..McpToolConfig::default()
});

let session = SessionUpdate {
    config: SessionUpdateConfig {
        tools: Some(vec![mcp]),
        tool_choice: Some(ToolChoice::Mode(ToolChoiceMode::Auto)),
        ..SessionUpdateConfig::default()
    },
};

client.send(ClientEvent::SessionUpdate { event_id: None, session: Box::new(session) }).await?;
# Ok(())
# }
```

## MCP approval flow (items)

```rust
use oai_rt_rs::protocol::models::{Item, ItemStatus};
use oai_rt_rs::protocol::client_events::ClientEvent;

# async fn demo(mut client: oai_rt_rs::RealtimeClient) -> oai_rt_rs::Result<()> {
let request = Item::McpApprovalRequest {
    id: Some("item_req_1".to_string()),
    status: Some(ItemStatus::InProgress),
    server_label: "weather".to_string(),
    name: "get_forecast".to_string(),
    arguments: r#"{"city":"Paris"}"#.to_string(),
};

client.send(ClientEvent::ConversationItemCreate {
    event_id: None,
    previous_item_id: None,
    item: Box::new(request),
}).await?;

let response = Item::McpApprovalResponse {
    id: Some("item_resp_1".to_string()),
    status: Some(ItemStatus::Completed),
    approval_request_id: "item_req_1".to_string(),
    approve: true,
    reason: None,
};

client.send(ClientEvent::ConversationItemCreate {
    event_id: None,
    previous_item_id: None,
    item: Box::new(response),
}).await?;
# Ok(())
# }
```

## Response creation

```rust
use oai_rt_rs::protocol::client_events::ClientEvent;
use oai_rt_rs::protocol::models::{ResponseConfig, InputItem, ContentPart, OutputModalities};

# async fn demo(mut client: oai_rt_rs::RealtimeClient) -> oai_rt_rs::Result<()> {
let response = ResponseConfig {
    output_modalities: Some(OutputModalities::Text),
    input: Some(vec![InputItem::Message {
        id: None,
        role: oai_rt_rs::protocol::models::Role::User,
        content: vec![ContentPart::InputText { text: "Hello".to_string() }],
    }]),
    ..ResponseConfig::default()
};

client.send(ClientEvent::ResponseCreate {
    event_id: None,
    response: Some(Box::new(response)),
}).await?;
# Ok(())
# }
```
