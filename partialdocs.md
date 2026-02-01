# **OpenAI Realtime API (GA) — Rust Crate Implementation Spec**

Scope: Everything required to implement a correct, production-grade Rust Realtime client: REST endpoints, WebSocket / WebRTC / SIP connect flows, session \+ conversation model, full client/server event protocol, audio/transcription semantics, interruption/cancel semantics, error \+ rate limits.

## **0\) Terminology and core objects**

### **Session**

A long-lived, mutable configuration object established when a client connects to Realtime. The server emits a `session.created` event immediately on connect, and emits `session.updated` after `session.update` ([Client events | OpenAI API Reference](https://platform.openai.com/docs/api-reference/realtime-client-events#:~:text=session)).

### **Conversation**

A sequence/graph of **Items** (messages, function calls, audio inputs, etc.) that constitute context for model inference. Items are created via `conversation.item.create` or via committing the audio buffer, and are streamed back via `conversation.item.added`/`conversation.item.done` and response events ([Audio events reference \- Azure OpenAI](https://learn.microsoft.com/en-us/azure/ai-foundry/openai/realtime-audio-reference?view=foundry-classic#:~:text=RealtimeClientEventConversationItemCreate%20The%20client%20,to%20the%20input%20audio%20buffer)).

### **Item**

A node in the conversation, e.g.:

* `message` (role: `user|assistant|system`)  
* tool call item(s)  
* audio input items (created by committing input audio buffer)  
  Items always carry `object: "realtime.item"` in server representation.

### **Response**

A model inference “run” initiated by `response.create` (or auto-created in VAD mode). Responses stream “output items” and content parts, and end with `response.done`.

## **1\) REST API endpoints (GA)**

### **1.1 Create ephemeral client secret (for browsers/mobile)**

**POST** `https://api.openai.com/v1/realtime/client_secrets`  
Use a standard API key server-side to mint a short-lived secret to hand to untrusted clients. The secret can be used to create sessions until it expires; the session may continue after secret expiry once started ([Client events | OpenAI API Reference](https://platform.openai.com/docs/api-reference/realtime-client-events#:~:text=session)).

Key request elements (high-level):

* `expires_after`: configurable expiration policy  
* `session`: optional session configuration pinned to the secret (can be overridden on connect).

### **1.2 WebRTC: create call (SDP exchange)**

**POST** `https://api.openai.com/v1/realtime/calls`  
Used to negotiate WebRTC with the model. You send SDP offer, receive SDP answer.

Two patterns exist in GA:

1. **Client-secret flow** (browser): client mints an ephemerral via your backend and then uses it to connect.  
2. **Unified server-side interface**: your server posts SDP+session config to `/v1/realtime/calls` with standard API key.

## **2\) WebSocket transport (most important for Rust)**

### **2.1 Primary WebSocket URL (server-to-server)**

`wss://api.openai.com/v1/realtime?model=gpt-realtime`  
Authentication:

* Header: `Authorization: Bearer <OPENAI_API_KEY>`

Browser-style subprotocol auth is also available:

* Subprotocols:  
  * `"realtime"`  
  * `"openai-insecure-api-key.<KEY>"`  
  * optional `"openai-organization.<ORG>"`, `"openai-project.<PROJECT>"`

### **2.2 WebSocket for SIP calls (attach to existing call)**

`wss://api.openai.com/v1/realtime?call_id={call_id}`  
Auth:

* Header: `Authorization: Bearer <OPENAI_API_KEY>`

### **2.3 Message framing**

* WebSocket messages are JSON-serialized **events** (strings).  
* Client sends “client events”.  
* Server emits “server events”.

## **3\) Session lifecycle and mutability**

### **3.1 First server event on connect: `session.created`**

* Always emitted as the first event after connection is established.  
* Contains default session config ([Client events | OpenAI API Reference](https://platform.openai.com/docs/api-reference/realtime-client-events#:~:text=session)).

### **3.2 Update session: `session.update` (client event)**

You may update any session field **except**:

* `model` (immutable)  
* `voice` (mutable only if no audio outputs have been produced yet) ([Client events | OpenAI API Reference](https://platform.openai.com/docs/api-reference/realtime-client-events#:~:text=session)).

Server replies with:

* `session.updated` containing full effective session config.

Clearing semantics:

* Clear `instructions` via `""`  
* Clear `tools` via `[]`  
* Clear `turn_detection` via `null`

Example:

```
{  
  "type": "session.update",  
  "session": {  
    "type": "realtime",  
    "instructions": "You are a creative assistant...",  
    "tools": [{ "type": "function", "name": "..." }]  
  }  
} 
```

## **4\) Conversation \+ Items model**

### **4.1 Create a user text message item**

Client event `conversation.item.create`:

```
{  
  "type": "conversation.item.create",  
  "item": {  
    "type": "message",  
    "role": "user",  
    "content": [{ "type": "input_text", "text": "hi" }]  
  }  
} 
```

Server will emit:

* `conversation.item.added` and `conversation.item.done` depending on timing and flow.

### **4.2 Retrieve an item**

Client: `conversation.item.retrieve`  
Server: `conversation.item.retrieved`

Client example:

```
{ "type":"conversation.item.retrieve", "event_id":"event_901", "item_id":"item_003" } 
```

### **4.3 Delete an item**

Client: `conversation.item.delete`  
Server: `conversation.item.deleted`

### **4.4 Truncate assistant audio (interruption sync)**

Client: `conversation.item.truncate`  
Purpose: when user interrupts, truncate already-sent-but-not-played assistant audio and remove transcript that user didn’t hear.

Example:

```
{  
  "type":"conversation.item.truncate",  
  "event_id":"event_678",  
  "item_id":"item_002",  
  "content_index": 0,  
  "audio_end_ms": 1500  
} 
```

Server: `conversation.item.truncated`.

## **5\) Audio input pipeline (WebSocket path)**

### **5.1 Append audio bytes: `input_audio_buffer.append`**

* Payload: base64 audio bytes.  
* No confirmation event is sent for each append (streaming with backpressure).

Example:

```
{ "type":"input_audio_buffer.append", "event_id":"event_456", "audio":"Base64EncodedAudioData" } 
```

### **5.2 Commit input audio: `input_audio_buffer.commit`**

* Creates a new **user message item** containing the audio.  
* Triggers input audio transcription if enabled.  
* Does **not** automatically trigger a model response unless VAD/auto-response is configured.

Server emits: `input_audio_buffer.committed` (and also item events).

### **5.3 Clear buffer: `input_audio_buffer.clear`**

Server emits: `input_audio_buffer.cleared`.

## **6\) VAD / turn detection (server side)**

Server VAD events:

* `input_audio_buffer.speech_started`  
* `input_audio_buffer.speech_stopped`  
* `input_audio_buffer.timeout_triggered` (idle timeout; can auto-commit empty segment and trigger a response) ([Realtime | OpenAI API Reference](https://platform.openai.com/docs/api-reference/realtime#:~:text=idle_timeout_ms)).

Transcription guide calls out VAD configuration knobs via session config (`audio.input.turn_detection`) such as `idle_timeout_ms`, `interrupt_response`, and `create_response`.

## **7\) Model inference: Response lifecycle**

### **7.1 Trigger inference: `response.create` (client event)**

* Starts a Response (model run).  
* In server VAD mode, responses can be auto-created.  
* Response can write to default conversation, or be “out-of-band” (`conversation:"none"`).

Examples:

```
{ "type":"response.create" } 
```

Advanced:

```
{  
  "type": "response.create",  
  "response": {  
    "conversation": "none",  
    "output_modalities": ["text"],  
    "instructions": "Provide a concise answer.",  
    "tools": []  
  }  
} 
```

### **7.2 Cancel: `response.cancel` (client event)**

Cancel an in-progress response (optionally by `response_id`).

Example:

```
{ "type":"response.cancel", "response_id":"resp_12345" } 
```

## **8\) Audio output control (WebRTC/SIP-specific)**

Client event:

* `output_audio_buffer.clear` — cut off current audio response (should be preceded by `response.cancel` to stop generation).

Server emits:

* `output_audio_buffer.started`  
* `output_audio_buffer.stopped`  
* `output_audio_buffer.cleared`

Client example:

```
{ "type":"output_audio_buffer.clear", "event_id":"optional_client_event_id" } 
```

## **9\) Full client event catalog (GA)**

Implement all of these as a Rust `enum ClientEvent` with `serde(tag="type")`:

1. `session.update` ([Client events | OpenAI API Reference](https://platform.openai.com/docs/api-reference/realtime-client-events#:~:text=session))  
2. `input_audio_buffer.append`  
3. `input_audio_buffer.commit`  
4. `input_audio_buffer.clear`  
5. `conversation.item.create`  
6. `conversation.item.retrieve`  
7. `conversation.item.truncate`  
8. `conversation.item.delete`  
9. `response.create`  
10. `response.cancel`  
11. `output_audio_buffer.clear` (WebRTC/SIP only)

## **10\) Full server event catalog (GA)**

Implement all of these as a Rust `enum ServerEvent` with `serde(tag="type")`:

### **10.1 Errors and session**

* `error`  
* `session.created` ([Client events | OpenAI API Reference](https://platform.openai.com/docs/api-reference/realtime-client-events#:~:text=session))  
* `session.updated`

### **10.2 Conversation item events**

* `conversation.item.added`  
* `conversation.item.done`  
* `conversation.item.deleted`  
* `conversation.item.retrieved`  
* `conversation.item.truncated`

### **10.3 Input audio buffer / VAD**

* `input_audio_buffer.committed`  
* `input_audio_buffer.cleared`  
* `input_audio_buffer.speech_started`  
* `input_audio_buffer.speech_stopped`  
* `input_audio_buffer.timeout_triggered` ([Realtime | OpenAI API Reference](https://platform.openai.com/docs/api-reference/realtime#:~:text=idle_timeout_ms))

### **10.4 Output audio buffer (WebRTC/SIP)**

* `output_audio_buffer.started`  
* `output_audio_buffer.stopped`  
* `output_audio_buffer.cleared`

### **10.5 Response lifecycle and streaming**

* `response.created`  
* `response.done`  
* `response.output_item.added`  
* `response.output_item.done`  
* `response.content_part.added`  
* `response.content_part.done`  
* `response.output_text.delta`  
* `response.output_text.done`  
* `response.output_audio.delta`  
* `response.output_audio.done`  
* `response.output_audio_transcript.delta`  
* `response.output_audio_transcript.done`

### **10.6 Transcription-specific conversation events**

* `conversation.item.input_audio_transcription.delta`  
* `conversation.item.input_audio_transcription.segment`  
* `conversation.item.input_audio_transcription.failed`  
* `conversation.item.input_audio_transcription.completed`

### **10.7 Rate limit updates**

* `rate_limits.updated`

### **10.8 MCP tool listing failures**

* `mcp_list_tools.failed`

## **11\) Event ordering & practical state machine rules**

### **11.1 On connect (WebSocket)**

Expect:

1. `session.created`  
2. optionally other server events depending on server defaults  
   Then you can send `session.update` to configure modalities/voice/tools/instructions.

### **11.2 Text turn (manual)**

1. `conversation.item.create` (user text item)  
2. `response.create`  
3. stream events:  
   * `response.created`  
   * `response.output_item.added`  
   * `response.content_part.added`  
   * `response.output_text.delta` (0..n times)  
   * `response.output_text.done`  
   * `response.content_part.done`  
   * `response.output_item.done`  
   * `response.done`

### **11.3 Audio turn (manual commit)**

1. many `input_audio_buffer.append`  
2. `input_audio_buffer.commit`  
3. (optionally wait for transcription events)  
4. `response.create` (unless VAD auto-response is enabled)

### **11.4 Audio turn (server VAD)**

* You stream `input_audio_buffer.append`.  
* Server emits `input_audio_buffer.speech_started` / `speech_stopped`.  
* Server commits automatically and may create a response automatically depending on VAD config.

### **11.5 Interruption**

If user interrupts assistant audio:

* Use `conversation.item.truncate` to sync what was actually heard and remove unheard transcript.  
  For WebRTC/SIP you can also:  
* `response.cancel` \+ `output_audio_buffer.clear` to stop audio generation and drain.

## **12\) SIP endpoints (telephony) — required for complete GA coverage**

From the SIP guide, key HTTP endpoints include (paths shown in examples):

* `POST /v1/realtime/calls/{call_id}/accept`  
* `POST /v1/realtime/calls/{call_id}/reject`  
* `POST /v1/realtime/calls/{call_id}/refer`  
  …and you then attach via WebSocket with `?call_id={call_id}`.

## **13\) Costs and rate limits (GA behavior)**

* Realtime costs accrue when a Response is created (manual or auto via VAD) and are based on modality tokens.  
* `rate_limits.updated` is emitted at the beginning of a response and reflects output token reservation, later adjusted.

## **14\) Rust crate architecture (recommended)**

### **14.1 Crate modules**

* `transport::ws` (tokio-tungstenite)  
* `protocol::{client, server, model}` (serde structs/enums)  
* `session` (mutable config mirror \+ update helpers)  
* `conversation` (item store \+ ordering by `previous_item_id`)  
* `audio` (input chunker, base64, optional resampling)  
* `runtime` (state machine \+ event dispatcher)

### **14.2 Strong typing strategy**

* `enum ClientEvent` and `enum ServerEvent` with `#[serde(tag="type")]`  
* Nested `Session`, `Item`, `ContentPart`, `Response` structs based on the API reference schemas and examples.

### **14.3 Concurrency model**

* 1 writer task: serializes client events  
* 1 reader task: parses server events and pushes into mpsc  
* user-facing API: async stream of `ServerEvent` \+ higher-level callbacks

### **14.4 Ordering**

* Use `previous_item_id` on item events to maintain consistent ordering when items are inserted mid-stream.

## **15\) Minimal “works” recipes (WS path)**

### **15.1 Text-only (manual)**

1. connect WS: `wss://api.openai.com/v1/realtime?model=gpt-realtime` with API key header  
2. optionally `session.update` (instructions/tools)  
3. `conversation.item.create` with `input_text`  
4. `response.create`  
5. handle `response.output_text.delta/done` and `response.done`

### **15.2 Audio-in (manual commit) \+ text/audio out**

1. `input_audio_buffer.append` chunks  
2. `input_audio_buffer.commit`  
3. `response.create`  
4. handle:  
   * `response.output_audio.delta/done`  
   * `response.output_audio_transcript.delta/done` (if enabled)

## **Appendix A: Canonical URLs / pages (authoritative)**

* Realtime overview: platform.openai.com/docs/guides/realtime  
* WebSocket guide: platform.openai.com/docs/guides/realtime-websocket  
* WebRTC guide: platform.openai.com/docs/guides/realtime-webrtc  
* Conversations guide: platform.openai.com/docs/guides/realtime-conversations  
* Transcription guide: platform.openai.com/docs/guides/realtime-transcription  
* Client events reference: platform.openai.com/docs/api-reference/realtime-client-events  
* Server events reference: platform.openai.com/docs/api-reference/realtime-server-events  
* Client secrets endpoint: platform.openai.com/docs/api-reference/realtime-sessions  
* Realtime REST (calls): platform.openai.com/docs/api-reference/realtime  
* SIP guide: platform.openai.com/docs/guides/realtime-sip
