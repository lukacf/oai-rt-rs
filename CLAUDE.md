# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**oai-rt-rs** is a Rust client library for the OpenAI Realtime API. It provides WebSocket connections, WebRTC signaling, and SIP telephony endpoints for building voice/audio applications with GPT-4o Realtime.

## Build & Test Commands

```bash
cargo build              # Debug build
cargo build --release    # Optimized release build
cargo test               # Run all tests
cargo test <test_name>   # Run single test (e.g., cargo test test_session_update)
```

## Architecture

```
┌─────────────────────────────────────────┐
│  RealtimeClient (public API)            │
│  - connect(), send(), next(), split()   │
└────────────┬────────────────────────────┘
             │
      ┌──────┴──────────┐
      │                 │
┌─────▼────────┐  ┌────▼──────────┐
│  transport/  │  │  protocol/    │
│  ws.rs       │  │  models.rs    │
│  rest.rs     │  │  client_*.rs  │
└──────────────┘  │  server_*.rs  │
                  └───────────────┘
```

### Core Modules

- **`src/lib.rs`** - `RealtimeClient` unified async client with `send()`, `next()`, and `split()` for concurrent send/receive patterns
- **`src/protocol/models.rs`** - Shared types: Session, Item, ContentPart, Response, Voice, AudioFormat, Tool, TurnDetection
- **`src/protocol/client_events.rs`** - `ClientEvent` enum (11 types): session.update, input_audio_buffer.*, conversation.item.*, response.*
- **`src/protocol/server_events.rs`** - `ServerEvent` enum (30+ types): session.*, conversation.*, response streaming events, audio I/O, transcription
- **`src/transport/ws.rs`** - WebSocket via tokio-tungstenite with TLS, supports call_id for SIP
- **`src/transport/rest.rs`** - REST endpoints: ephemeral tokens, WebRTC SDP, SIP accept/reject/refer

### Key Patterns

- **Tagged serde enums** - `ClientEvent` and `ServerEvent` use `#[serde(tag = "type")]` for JSON serialization
- **Async/await with Tokio** - All I/O is async; client can be split for concurrent usage
- **Base64 audio** - Audio transmitted as base64-encoded strings in JSON (not binary frames)
- **Optional field handling** - Uses `#[serde(skip_serializing_if = "Option::is_none")]` throughout
- **Error type** - Uses `Box<dyn std::error::Error + Send + Sync>` for composability

### Split Client Pattern

```rust
let (mut sender, mut receiver) = client.split();
tokio::spawn(async move {
    while let Some(evt) = receiver.next().await { /* handle events */ }
});
sender.send(event).await?;
```

## Protocol Reference

Reference documentation in `partialdocs.md` and `OPENAI_REALTIME_SPEC.md` for:
- Full event catalog and examples
- Session lifecycle and mutability rules
- Audio input/output pipelines
- VAD/turn detection configuration
