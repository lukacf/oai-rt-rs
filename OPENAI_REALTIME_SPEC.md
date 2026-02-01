# OpenAI Realtime API Specification (GA & SIP Compatible)

This document provides a comprehensive specification for implementing a Rust client for the OpenAI Realtime API. It supplements the initial partial documentation with full JSON schemas, data models, and protocol details derived from official General Availability (GA) documentation.

## 1. Core Data Models

### 1.1 Session Configuration (`Session`)

The `Session` object is used to configure the connection. It is mutable via `session.update`.

```json
{
  "id": "sess_...",
  "object": "realtime.session",
  "type": "realtime",            // "realtime" or "transcription"
  "model": "gpt-realtime",
  "output_modalities": ["audio"], // "audio" or "text", but not both simultaneously.
  "instructions": "System instructions...",
  "voice": "alloy",              // alloy, echo, shimmer, ash, ballad, coral, sage, verse, marin, cedar
  "audio": {
      "input": {
          "format": { "type": "audio/pcm", "rate": 24000 },
          "turn_detection": { 
              "type": "server_vad",
              "threshold": 0.5,
              "prefix_padding_ms": 300,
              "silence_duration_ms": 500,
              "idle_timeout_ms": 500,
              "create_response": true
          }
      },
      "output": {
          "format": { "type": "audio/pcm" },
          "voice": "alloy",
          "speed": 1.0
      }
  },
  "tools": [
    {
      "type": "function",
      "name": "get_weather",
      "description": "Get current weather",
      "parameters": { "type": "object", "properties": { ... } }
    },
    {
      "type": "mcp",
      "server_label": "weather-server"
    }
  ],
  "tool_choice": "auto",
  "temperature": 0.8,
  "max_output_tokens": "inf"
}
```

### 1.2 Conversation Item (`Item`)

Items form the conversation history. Types include `message`, `function_call`, `function_call_output`, `mcp_call`, and `mcp_list_tools`.

**Message Item:**
```json
{
  "id": "item_...",
  "object": "realtime.item",
  "type": "message",
  "status": "completed",
  "role": "user",
  "content": [
    {
      "type": "input_text",
      "text": "Hello"
    },
    {
      "type": "input_image",
      "image_url": "data:image/jpeg;base64,...",
      "detail": "high"
    },
    {
      "type": "output_audio",
      "audio": "base64...",
      "transcript": "Hi there!",
      "format": { "type": "audio/pcm", "rate": 24000 }
    }
  ]
}
```

## 2. WebSocket Event Catalog

### 2.1 Client Events (Sent by Client)

Common field: `event_id` (optional string).

- `session.update`: Update configuration.
- `input_audio_buffer.append`: Stream audio (base64).
- `input_audio_buffer.commit`: Signal end of audio segment.
- `input_audio_buffer.clear`: Clear buffer.
- `conversation.item.create`: Add item to history.
- `conversation.item.retrieve`: Request full item data (including audio bytes).
- `conversation.item.truncate`: Truncate audio output.
- `conversation.item.delete`: Remove item.
- `response.create`: Trigger generation.
- `response.cancel`: Stop generation.

### 2.2 Server Events (Sent by Server)

- `conversation.item.retrieved`: Full item details.
- `input_audio_buffer.dtmf_event_received`: SIP DTMF tone detection.
- `response.output_text.delta/done`: Streaming text.
- `response.output_audio.delta/done`: Streaming audio.
- `response.mcp_call_arguments.delta/done`: MCP tool streaming.

## 3. WebRTC & REST API

### 3.1 Create Ephemeral Client Secret (GA)
**POST** `/v1/realtime/client_secrets`
Body: `{"session": { ... }}`
Response: `{"value": "...", "expires_at": ..., "session": { ... }}`

### 3.2 WebRTC Handshake
**POST** `/v1/realtime/calls`

**Options:**
- **Direct (Browser)**: Body is raw SDP, `Content-Type: application/sdp`. Uses ephemeral key.
- **Unified (Server)**: Multipart form data with `sdp` and `session` parts. Uses standard API key.

### 3.3 SIP Control
- **Accept**: `POST /v1/realtime/calls/{call_id}/accept` (Body: Session configuration object)
- **Refer**: `POST /v1/realtime/calls/{call_id}/refer` (Body: `{"target_uri": "sip:..."}`)
