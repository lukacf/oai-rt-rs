Realtime API
============

Build low-latency, multimodal LLM applications with the Realtime API.

The OpenAI Realtime API enables low-latency communication with [models](/docs/models) that natively support speech-to-speech interactions as well as multimodal inputs (audio, images, and text) and outputs (audio and text). These APIs can also be used for [realtime audio transcription](/docs/guides/realtime-transcription).

Voice agents
------------

One of the most common use cases for the Realtime API is building voice agents for speech-to-speech model interactions in the browser. Our recommended starting point for these types of applications is the [Agents SDK for TypeScript](https://openai.github.io/openai-agents-js/guides/voice-agents/), which uses a [WebRTC connection](/docs/guides/realtime-webrtc) to the Realtime model in the browser, and [WebSocket](/docs/guides/realtime-websocket) when used on the server.

```js
import { RealtimeAgent, RealtimeSession } from "@openai/agents/realtime";

const agent = new RealtimeAgent({
    name: "Assistant",
    instructions: "You are a helpful assistant.",
});

const session = new RealtimeSession(agent);

// Automatically connects your microphone and audio output
await session.connect({
    apiKey: "<client-api-key>",
});
```

[

Voice Agent Quickstart

Follow the voice agent quickstart to build Realtime agents in the browser.

](https://openai.github.io/openai-agents-js/guides/voice-agents/quickstart/)

To use the Realtime API directly outside the context of voice agents, check out the other connection options below.

Connection methods
------------------

While building [voice agents with the Agents SDK](https://openai.github.io/openai-agents-js/guides/voice-agents/) is the fastest path to one specific type of application, the Realtime API provides an entire suite of flexible tools for a variety of use cases.

There are three primary supported interfaces for the Realtime API:

[

WebRTC connection

Ideal for browser and client-side interactions with a Realtime model.

](/docs/guides/realtime-webrtc)[

WebSocket connection

Ideal for middle tier server-side applications with consistent low-latency network connections.

](/docs/guides/realtime-websocket)[

SIP connection

Ideal for VoIP telephony connections.

](/docs/guides/realtime-sip)

Depending on how you'd like to connect to a Realtime model, check out one of the connection guides above to get started. You'll learn how to initialize a Realtime session, and how to interact with a Realtime model using client and server events.

API Usage
---------

Once connected to a realtime model using one of the methods above, learn how to interact with the model in these usage guides.

*   **[Prompting guide](/docs/guides/realtime-models-prompting):** learn tips and best practices for prompting and steering Realtime models.
*   **[Managing conversations](/docs/guides/realtime-conversations):** Learn about the Realtime session lifecycle and the key events that happen during a conversation.
*   **[Webhooks and server-side controls](/docs/guides/realtime-server-controls):** Learn how you can control a Realtime session on the server to call tools and implement guardrails.
*   **[Managing costs](/docs/guides/realtime-costs):** Learn how to monitor and optimize your usage of the Realtime API.
*   **[Realtime audio transcription](/docs/guides/realtime-transcription):** Transcribe audio streams in real time over a WebSocket connection.

Beta to GA migration
--------------------

There are a few key differences between the interfaces in the Realtime beta API and the recently released GA API. Expand the topics below for more information about migrating from the beta interface to GA.

Beta header

For REST API requests, WebSocket connections, and other interfaces with the Realtime API, beta users had to include the following header with each request:

```text
OpenAI-Beta: realtime=v1
```

This header should be removed for requests to the GA interface. To retain the behavior of the beta API, you should continue to include this header.

Generating ephemeral API keys

In the beta interface, there were multiple endpoints for generating ephemeral keys for either Realtime sessions or transcription sessions. In the GA interface, there is only one REST API endpoint used to generate keys - [`POST /v1/realtime/client_secrets`](/docs/api-reference/realtime-sessions/create-realtime-client-secret).

To create a session and receive a client secret you can use to initialize a WebRTC or WebSocket connection on a client, you can request one like this using the appropriate session configuration:

```javascript
const sessionConfig = JSON.stringify({
    session: {
        type: "realtime",
        model: "gpt-realtime",
        audio: {
            output: { voice: "marin" },
        },
    },
});

const response = await fetch("https://api.openai.com/v1/realtime/client_secrets", {
    method: "POST",
    headers: {
        Authorization: `Bearer ${apiKey}`,
        "Content-Type": "application/json",
    },
    body: sessionConfig,
});

const data = await response.json();
console.log(data.value); // e.g. ek_68af296e8e408191a1120ab6383263c2
```

These tokens can safely be used in client environments like browsers and mobile applications.

New URL for WebRTC SDP data

When initializing a WebRTC session in the browser, the URL for obtaining remote session information via SDP is now `/v1/realtime/calls`:

```javascript
const baseUrl = "https://api.openai.com/v1/realtime/calls";
const model = "gpt-realtime";
const sdpResponse = await fetch(baseUrl, {
    method: "POST",
    body: offer.sdp,
    headers: {
        Authorization: `Bearer YOUR_EPHEMERAL_KEY_HERE`,
        "Content-Type": "application/sdp",
    },
});

const sdp = await sdpResponse.text();
const answer = { type: "answer", sdp };
await pc.setRemoteDescription(answer);
```

New event names and shapes

When creating or [updating](/docs/api-reference/realtime_client_events/session/update) a Realtime session in the GA interface, you must now specify a session type, since now the same client event is used to create both speech-to-speech and transcription sessions. The options for the session type are:

*   `realtime` for speech-to-speech
*   `transcription` for realtime audio transcription

```javascript
import WebSocket from "ws";

const url = "wss://api.openai.com/v1/realtime?model=gpt-realtime";
const ws = new WebSocket(url, {
    headers: {
        Authorization: "Bearer " + process.env.OPENAI_API_KEY,
    },
});

ws.on("open", function open() {
    console.log("Connected to server.");

    // Send client events over the WebSocket once connected
    ws.send(
        JSON.stringify({
            type: "session.update",
            session: {
                type: "realtime",
                instructions: "Be extra nice today!",
            },
        })
    );
});
```

Configuration for input modalities and other properties have moved as well, notably output audio configuration like model voice. [Check the API reference](/docs/api-reference/realtime_client_events) for the latest event shapes.

```javascript
ws.on("open", function open() {
    ws.send(
        JSON.stringify({
            type: "session.update",
            session: {
                type: "realtime",
                model: "gpt-realtime",
                audio: {
                    output: { voice: "marin" },
                },
            },
        })
    );
});
```

Finally, some event names have changed to reflect their new position in the event data model:

*   **`response.text.delta` → `response.output_text.delta`**
*   **`response.audio.delta` → `response.output_audio.delta`**
*   **`response.audio_transcript.delta` → `response.output_audio_transcript.delta`**

New conversation item events

For `response.output_item`, the API has always had both `.added` and `.done` events, but for conversation level items the API previously only had `.created`, which by convention is emitted at the start when the item added.

We have added a `.added` and `.done` event to allow better ergonomics for developers when receiving events that need some loading time (such as MCP tool listing or input audio transcriptions if these were to be modeled as items in the future).

Current event shape for conversation items added:

```javascript
{
    "event_id": "event_1920",
    "type": "conversation.item.created",
    "previous_item_id": "msg_002",
    "item": Item
}
```

New events to replace the above:

```javascript
{
    "event_id": "event_1920",
    "type": "conversation.item.added",
    "previous_item_id": "msg_002",
    "item": Item
}
```

```javascript
{
    "event_id": "event_1920",
    "type": "conversation.item.done",
    "previous_item_id": "msg_002",
    "item": Item
}
```

Input and output item changes

### All Items

Realtime API sets an `object=realtime.item` param on all items in the GA interface.

### Function Call Output

`status` : Realtime now accepts a no-op `status` field for the function call output item param. This aligns with the Responses API implementation.

### Message

**Assistant Message Content**

The `type` properties of output assistant messages now align with the Responses API:

*   `type=text` → `type=output_text` (no change to `text` field name)
*   `type=audio` → `type=output_audio` (no change to `audio` field name)
