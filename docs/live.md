# Public GPT-Live

`oai_rt_rs::live` is the public [OpenAI Live API](https://developers.openai.com/api/reference/resources/live). It is not the Realtime API,
and it does not use `experimental::gpt_live`, [ChatGPT](https://chatgpt.com) OAuth, private bootstrap
endpoints, or Alpha/Beta headers. The experimental adapter remains isolated
behind its existing feature.

## Primary WebSocket

```rust,no_run
use oai_rt_rs::live::{ClientEvent, Command, Field, LiveClient, Nullable, SessionConfig};

# async fn example(key: &str) -> oai_rt_rs::live::Result<()> {
let client = LiveClient::new(key)?;
let mut connection = client.connect(SessionConfig {
    instructions: Field::Value("Keep spoken answers short.".into()),
    ..SessionConfig::default()
}).await?;

// connect waits for session.started; that event remains in the event stream.
connection.send(ClientEvent::new(Command::ThinkingAppend {
    content: "The application is ready.".into(),
    delegation_id: Nullable(None), // required JSON null, not an omitted field
})).await?;

let sender = connection.sender();
sender.send_audio(&[0, 0]).await?; // one raw PCM16 sample, NOT a WAV file

while let Some(frame) = connection.next_event().await? {
    // Consume typed frame.event. frame.raw retains unknown fields/events.
    // Do not log raw audio, transcripts, instructions, or tool arguments.
    if matches!(frame.event, oai_rt_rs::live::ServerEvent::Closed { .. }) {
        break;
    }
}
# Ok(())
# }
```

The primary endpoint is `wss://api.openai.com/v1/live/sessions`, with **no model
query parameter**. The first command is `session.start`; its `session.model`
defaults to `gpt-live-1`. The session object has **no `type: "live"` field**.
`LiveClient::connect` sends this first command and waits for `session.started`.

Use `ClientOptions` for standard organization/project headers and explicit
timeouts, event-size limits, and queue bounds. API keys stay on trusted servers.
Redirects are disabled. The client never retries a request automatically.

## WebRTC, sideband, forks, and calls

For browser/native peers, POST SDP signaling through the trusted Rust backend:

```rust,no_run
use oai_rt_rs::live::{CreateRequest, LiveClient, SessionConfig, WebRtcTransport};
# async fn signal(key: &str, offer: String) -> oai_rt_rs::live::Result<()> {
let client = LiveClient::new(key)?;
let created = client.create_webrtc(&CreateRequest {
    session: SessionConfig::default(),
    transport: WebRtcTransport::WebRtc { sdp: offer },
}).await?;
let sideband = client.attach(&created.session.id).await?;
let answer_sdp = created.transport.sdp();
# let _ = (sideband, answer_sdp);
# Ok(())
# }
```

Creation/fork responses are HTTP 201 JSON with **only `session.id`** plus
`transport: {type: "webrtc", sdp: ...}`. They are not full `SessionSnapshot`s.
Apply the answer in the media peer; do not send `session.start` again on its
`oai-events` data channel. Omit `audio.format`: WebRTC negotiates media.

Attach early at `/v1/live/sessions/{id}/attach`; there is no historical replay.
`session.started` on sideband can precede peer/media readiness. Neither it nor a
context ACK proves audio playback is ready. `SidebandOptions::graceful_close`
only opts into the WebSocket closing handshake **when the session ends**. Its
default `None` omits the query; the server may enable the behavior itself.
Dropping a sideband disconnects that observer, not an implicit `session.close`;
the underlying call can continue.

`client.data_channel` permission lists distinguish omitted defaults, `"all"`,
and `[]` (deny all). Lists are bounded to 256 entries. Named client events must
match the schema's lowercase dotted-name pattern (or `error`/`info`). A
`response.event` server selector requires a nested `response_event` selector.
Trusted sidebands are not restricted by frontend permissions. Browser permission
rejections may appear only on the data channel, not the sideband.

`LiveClient::fork(source, ForkSessionConfig)` connects to the fork WebSocket and
sends its distinct first `session.start {session: ...}`. Empty overrides are
valid; new model, original instructions/input, voice changes and mode switches
are not. WS forks reset the codec to PCM16/24k unless overridden and discard
frontend permissions. `fork_webrtc(source, &ForkRequest)` posts a new SDP offer;
its optional session overrides preserve permissions when absent, and forbid
audio overrides. Both return a **new** session, not resumed application work.

`download_content(id)` streams a completed stored recording without persisting
it. `read_all(max_bytes)` is an explicitly bounded convenience. The endpoint
requires `live_` followed by 1-128 URL-safe identifier characters. Other control
IDs remain opaque and are encoded as one path segment.

Call controls use `accept_call`, `reject_call`, `refer_call`, and `hangup`.
SIP acceptance deliberately uses `AcceptRequest`/`SipAcceptSession`, whose
required `type: "live"` is confirmed by the full `OpenAPI`; this field is **not**
valid on primary/RTC startup. SIP negotiates audio and has no frontend data
channel permissions. Rejection status is an integer from 300 through 699.
Refer takes a nonblank destination string; hangup has no request body. All four
expect HTTP 200 with an empty body, not JSON. A hangup receipt is not final usage.

`IncomingWebhook` retains current `live.transport.incoming` and deprecated
`live.call.incoming` bodies. Verify signatures against the **original raw body**
and deduplicate deliveries in the application before accepting/rejecting.
`Codec::decode_webhook` is JSON validation, not authentication. SIP header names,
values, repetition, and order are untrusted metadata. No public outbound-SIP
creation or recording-deletion endpoint is invented here.

### Audio and captions

One immutable `session.audio.format` controls both primary input and output:

| Format | Rate | Encoding |
| --- | --- | --- |
| `audio/pcm` | 24,000 Hz (default) or 16,000 Hz | Mono signed PCM16, little-endian |
| `audio/pcmu` | 8,000 Hz | Mono G.711 mu-law |
| `audio/pcma` | 8,000 Hz | Mono G.711 A-law |

Send raw bytes in order, paced at the configured sample rate, including silence.
No WAV/container headers, manual commits, audio-buffer clearing, voice
`response.create`, cancel, or truncate commands exist on this API. The library
validates complete PCM samples but does not resample or convert codecs.
`send_audio_paced` is useful for bounded prerecorded/synthetic samples; real media
capture/playback and their buffering belong to the application.

`ServerFrame::audio` decodes consumable raw audio without throwing away other
events. Supply the connection role and primary format. **Sideband reflected audio
is always PCM16LE at 24 kHz**, regardless of negotiated media or primary format.
Reflected input is before model-input muting; receiving it is not permission to
send audio over sideband.

Primary output audio deltas have no authoritative timestamp, event ID, response
ID, item ID, or audio-done event. Sideband output has `start_ms` and `end_ms`. One
shared reference example shows timestamps on primary output; the transport guide
and optional schema fields are the boundary used here. Raw fields remain available,
but the primary audio helper does not invent playback timing from them.

Transcript `delta` strings retain whitespace. Their numeric (possibly fractional)
`start_ms`/`end_ms` describe session-relative half-open intervals. Input and output
can overlap. There are **no authoritative turn IDs or completed-turn events**.
Neither transcript boundaries nor audio generation prove that playback completed.

### Full-duplex operation and close

`connection.split()` returns a cloneable `LiveSender` and one `LiveReceiver`.
Queues are bounded. A single driver owns the wire order and continues servicing
commands under event backpressure. No success acknowledgment is awaited for
audio append or backend item creation.

Send completion confirms a transport write, not provider acceptance or exactly-once
delivery. Cancelling a send after enqueueing may race with a write. An I/O failure
during writing returns `AmbiguousWrite`; **do not blindly retry**. Event IDs are
correlation identifiers, not idempotency keys.

`connection.close(timeout, observe)` requests `session.close`, forwards remaining
events to the callback, and returns the terminal `session.closed` frame with final
usage. New commands are rejected after closing starts. Active backend work may
finish, but a pending function-result continuation cannot be submitted then.
Dropping the receiver aborts the driver and releases the socket; it is not graceful
close. A disconnected transport without a terminal event yields `UnconfirmedClose`,
not successful finalization. A close deadline aborts the remaining transport.

Malformed nonterminal events are surfaced as `Error::MalformedEvent`, including
an explicit-only raw payload, without automatically ending the reader. Use
`close_with_events` to observe `Result<ServerFrame>` values and deliberately
continue draining after a decode error. Ordinary `close` is fail-fast on such
errors. A malformed final event never confirms usage; a later valid final event
can. Queue backpressure does not discard events or grow an unbounded raw-event
buffer. Frame-capacity loss is explicit `ContinuityLost` plus unconfirmed usage.

HTTP failures preserve status, full response headers, request ID, retry hints,
and a bounded body prefix. `HttpBodyIssue` distinguishes truncation from failed
body reads, without losing known HTTP metadata. Request/event/error Debug output
redacts content and credentials; raw bodies require explicit access.

Failed WebSocket upgrades expose only the bytes buffered with the handshake.
They remain `Unconfirmed` unless a single ASCII-digits `Content-Length` exactly
matches that buffer and there is no `Transfer-Encoding`. No second request is
made to guess the body. Delegation validation retains only the immutable mode,
not an unbounded history of delegation IDs; the provider validates unknown IDs.

`session.closed` reasons are `close_requested`, `expired`, `content`,
`remote_hangup`, and `connection_lost`. **The snapshot still says `status: "active"`**:
the event, not that snapshot field, is terminal.

## Configuration and validation

`Field<T>` represents three states: `Absent`, `Null`, and `Value(T)`. This matters
for sparse backend updates. `Nullable<T>` represents a required nullable field,
such as context `delegation_id`. Optional non-nullable fields reject explicit
JSON null.

Model, original instructions, initial input, audio/voice, storage, frontend
permissions, and delegation mode are startup-only. `session.update` changes only
supported `delegation.responses` settings. Omitted settings retain their values.
`delegation: null` selects client delegation; it cannot reset a running Responses
session or switch its mode.

Startup history accepts at most 128 messages, with roles `developer`, `user`, or
`assistant` and exactly one text part each. Developer/user messages use
`input_text`; assistant messages use `text` or `output_text`. No `system` role,
images, or audio belongs in this startup history.

The provider imposes token limits of 8,192 rendered startup-history tokens,
16,384 instruction tokens, and 500 tokens per context append. This crate checks
structural constraints, **not a guessed token count**. Provider tokenizer errors
remain visible. It also checks documented numeric bounds and UTF-8 character
counts for correlation IDs/selectors/custom voice IDs.

The full `OpenAPI` requires **integer** backend token limits; the rendered
reference flattens this distinction to “number.” `max_output_tokens` therefore
uses `u64`, while session-relative time and usage retain fractional values.
Provider errors have been observed with `param: null`; the lifecycle guide also
permits `code: null`. These inbound cases are supported narrowly. Required
`code` cannot be omitted, and ACK `client_event_id` remains non-nullable.

`Codec::decode_client` rejects unknown outgoing fields and duplicate JSON keys,
including nested configuration. `decode_request` provides equivalent strict
decoding for typed HTTP/config inputs. Inbound known events are typed; malformed
known events fail instead of being mislabeled unknown. `ServerFrame::raw` retains
future event bodies and extra fields for forward compatibility. The default
16 MiB event-size limit is a configurable local resource bound, not a claimed
provider limit.

## Delegation

Client delegation emits `session.delegation.created` with an opaque ID, a numeric
offset, and `target: "client"`. It carries **no task text, arguments, handoff ID,
or completed user turn**. Build a task from application-owned transcripts/state.
Repeated context appends may reuse that client delegation ID.

Use `InstructionsAppend` for trusted steering (which may interrupt),
`ThinkingAppend` for quiet facts/progress, and `CommentaryAppend` for information
to say aloud. All require `delegation_id`: use `Nullable(None)` for unsolicited
context, or a known client delegation ID. A function call ID or Responses
delegation ID is not a client delegation ID. Sidebands do not replay past
delegations, so unknown IDs are ultimately validated by the provider.

Appended ACKs have optional non-null `client_event_id` and required numeric
`start_ms`/`end_ms`, possibly equal. They have no delegation ID. These are estimated
context-injection intervals, **not consumption, speech, or playback receipts**;
frame stalls may delay the ACK.

Responses delegation configures a separate backend model, instructions, function
or web-search tools, tool choice, parallel tool calls, reasoning, text verbosity,
service tier, and `max_output_tokens` (at least 16). It is not the full standalone
Responses creation API. The schema's MCP tool-choice selector does not authorize
arbitrary MCP tool registration.

`ResponseInputItem` covers the shared input-item schema, including typed text,
images and function outputs. Its additional shared item/tool types describe wire
representations, not permission to register otherwise unsupported Live tools.
`ServerFrame::response_event` decodes nested lifecycle, item, function-argument,
and text events. Other nested events retain their raw maps.

Shared outbound types enforce recursive filters and object-only schemas without
accepting arbitrary JSON as a fallback. File-search result counts are 1-50;
ranking thresholds and result scores are 0-1. Image compression is 0-100 and
partial-image counts 0-3. The full reachable input graph was checked for numeric,
list, record, string, and identifier bounds. No extra range is invented for
hybrid-search weights or fields with no documented bound. Function-result text
has a 10,485,760-character limit; a content array instead applies each part's
own constraints. Nested arbitrary values remain supported inside object schemas.

Listen inside the `response.event` wrapper, preserving its outer delegation ID.
Only completed `response.output_item.done` function items are actionable; partial
argument deltas are not. Track complete `call_id`, `name`, and `arguments`.
Forwarded lifecycle snapshots deliberately clear `output` and `tools`, set
`instructions` to null, and omit input. Consequently,
**`response.completed.output: []` does not mean there were no function calls**.

`FunctionCallTracker::observe(outer_delegation_id, &event)` returns
`ResponseAttribution::Owned(ResponseKey)`, `Unowned`, or `Ambiguous`. Feed the
complete ordered stream, including item-added/item-done and lifecycle events.
The key includes both the nested response ID and available outer delegation.
Previously bound item IDs survive overlapping responses and late duplicates.
An unbound item can use a known scope's sole open response, never an arbitrary
last-active response. Null/omitted scopes are unowned, not a shared catch-all
stream. Unknown and ambiguous facts must be handled explicitly by the caller.

`calls(&key)` exposes finished **items**, possibly an incomplete batch.
`ready_calls(&key)` requires that exact response's creation and successful
completion, all observed items finished, and no uncertainty. `Some(&[])` is a
confirmed empty set; `None` is not. Failed/incomplete responses, missing start,
unresolved ownership and stream loss never become ready. Call `mark_uncertain`
after a dropped/malformed event. Retain errors and explicit unfinished state;
remove tracked responses only when late events no longer need their bindings.
The helper neither executes functions nor sends continuations.

Submit all pending function results with `response.item.create`, then explicitly
send `response.create`. Item creation has no standalone success ACK and does not
automatically continue. `response.create` has no model override, request body, or
delegation ID; it continues configured backend work, not voice speech.

Application-owned tools still require authorization, argument validation, and
appropriate user confirmation. The library never executes a tool automatically.
Images may be sent to a delegated vision-capable backend, not directly to the
Live audio frontend.

### Observed provider failure during continuation

Synthetic native Rust and independent raw Python sessions have intermittently
received `invalid_request_error` / “Responses handoff incomplete” correlated to
an explicit continuation, **after a new backend response was already reported
created/in-progress**. The preceding function response was completed and its
output was submitted; the original diagnostic also updated tool choice and
waited for the matching update ACK. Equivalent requests have also succeeded.
This is not a categorical unsupported-configuration claim or a passed scenario.

An error must not be interpreted as proof that no work was admitted. Preserve
response/delegation identity and pending-call state, keep observing later
terminal events, and obtain final session usage. The client adds no hidden
delays, retries, or automatic tool execution to mask this behavior.

## Usage and persistence

`usage.seconds` is cumulative: replace older snapshots instead of summing them.
`context_window.usage_ratio` is optional. The frontend normally has a 128k context;
the provider may roll the internal voice engine above 90% usage while keeping the
**same public session** and up to 8,192 tokens of history. This is not an
application task restart.

Voice pricing is $0.05/minute, billed per second, including silence/backend waits.
WebRTC's 15-second initialization is credited against total duration, not added
to it. Backend usage is separate. The crate reports observations; it does not
implement billing, quotas, or execution policy.

Storage defaults to false, requires project enablement and a non-ZDR project,
and retains recordings for 30 days. A fork from a finished stored session creates
a **new** session ID; it does not resume application jobs or authorize reexecuting
tools. Downloaded content is stereo WAV: input on the left, output on the right.

## Source contract

The implementation targets the public release announced September 10, 2026,
verified against official sources September 11, 2026:

- [Changelog](https://developers.openai.com/api/docs/changelog)
- [GPT-Live 1 model](https://developers.openai.com/api/docs/models/gpt-live-1)
- [Primary WebSocket schema](https://developers.openai.com/api/reference/resources/live/primary-websocket)
- [Sideband WebSocket schema](https://developers.openai.com/api/reference/resources/live/sideband-websocket)
- [Session lifecycle](https://developers.openai.com/api/docs/guides/live-conversations)
- [Delegation and tools](https://developers.openai.com/api/docs/guides/live-delegation)
- [Migration](https://developers.openai.com/api/docs/guides/live-migration)
- [Prompting](https://developers.openai.com/api/docs/guides/live-prompting)
- [Primary audio transport](https://developers.openai.com/api/docs/guides/voice-websockets?api=live)
- [WebRTC](https://developers.openai.com/api/docs/guides/voice-webrtc?api=live)
- [Server controls](https://developers.openai.com/api/docs/guides/voice-server-controls?api=live)
- [SIP](https://developers.openai.com/api/docs/guides/voice-sip?api=live)
- [Latency and cost](https://developers.openai.com/api/docs/guides/voice-latency-cost?api=live)
- [Official documentation MCP / endpoint OpenAPI](https://developers.openai.com/mcp)
- [Complete pinned OpenAPI](https://raw.githubusercontent.com/openai/openai-openapi/38170fdddbb6a1813eae6c6587ee17cf2987185b/openapi.json)

The complete spec revision is `38170fdddbb6a1813eae6c6587ee17cf2987185b`
(September 11, 2026), SHA-256
`3d6223349eadfd937624b9e6b8abf596ec2f680a1a367889cf6a6f924e568127`.
It resolves components omitted by per-endpoint MCP results. Several rendered
HTTP reference links were unavailable; relative MCP endpoint paths and the full
spec provide the authoritative HTTP shapes. The dedicated sideband union omits
some reflected-audio/DTMF events that its guide and broader schemas document;
the implementation accepts the documented broader event surface.

## Reproducible checks

Deterministic suites are `live_models`, `live_codec`, `live_calls`, `live_fork`,
`live_http`, `live_responses`, `live_ws`, `live_ws_options`, and
`live_ws_adversarial`, plus `live_ws_http_errors`. They cover wire forms, null/absent distinctions, unknown
fields, bounds, HTTP failures, complete function items, ordering, backpressure,
cancellation, malformed-event draining, final usage and credential redaction.

| Public contract | Implementation | Deterministic coverage / native probe |
| --- | --- | --- |
| Primary, configuration, context, continuous audio, usage/close | `models`, `events`, `codec`, `ws` | `live_models`, `live_codec`, `live_ws*`; `live_smoke`, `live_formats_smoke` |
| Client delegation | `events`, `ws` | `live_ws`; `live_client_delegation_smoke` |
| Managed Responses, all shared input alternatives/tools, scoped calls | `responses`, `events` | `live_responses`; `live_responses_smoke` |
| WebRTC, sideband and frontend permissions | `rest`, `models`, `ws` | `live_http`, `live_ws_options`; real-peer `live_webrtc_smoke` |
| Stored content and both fork transports | `rest`, `fork`, `ws` | `live_http`, `live_fork`; `live_storage_smoke`, real-peer `--fork` |
| SIP controls and incoming webhook bodies | `sip`, `rest`, `codec` | `live_calls`, `live_http`, `live_codec`; carrier live qualification unavailable |

```bash
cargo test --all-features --all-targets
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
RUSTDOCFLAGS=-Dwarnings cargo doc --no-deps --all-features
cargo package --all-features
```

Live examples are **explicit, billable, bounded** probes requiring a server-side
`OPENAI_API_KEY`; missing credentials fail rather than silently skipping.
Use synthetic input only. They never store recording bytes or credentials.

```bash
cargo run --example live_smoke
cargo run --example live_formats_smoke
cargo run --example live_client_delegation_smoke -- synthetic-pcm16le-24k.raw
cargo run --example live_responses_smoke
cargo run --example live_storage_smoke
cargo build --example live_webrtc_smoke
python scripts/live_webrtc_smoke.py --binary target/debug/examples/live_webrtc_smoke
python scripts/live_webrtc_smoke.py --binary target/debug/examples/live_webrtc_smoke --restrict-browser
python scripts/live_webrtc_smoke.py --binary target/debug/examples/live_webrtc_smoke --fork
```

The managed example's standard workflow uses typed text parts, serial tools,
20 ms audio, and an acknowledged sparse update before work. It collects all
function items until the matching response completes before submitting results.
`--update-during-handoff` preserves the separate intermittent-failure diagnostic
(forced initial tool, parallel tools, a tool-choice update between result and
continuation); `--100ms-audio` preserves its alternate pacing. Neither option is
a retry or a hidden workaround. Reports distinguish continuation sent, response
created, and terminal status, including partial admission after an error.

The peer harness requires `aiortc` and `av`; Rust performs signaling and sideband
control, while the Python peer verifies real media, meaningful voiced energy,
captions and finalization. Packet receipt/comfort noise alone is not a speech
pass. Recording probes verify stereo PCM data in memory and create a distinct WS
fork; the peer harness separately exercises the HTTP/RTC fork.

Carrier-backed SIP acceptance/rejection/transfer was **not live-qualified**:
no test trunk/webhook fixture was available, and no phone numbers, project
settings or paid services were provisioned. Its typed contracts and positive/
negative HTTP behavior are covered deterministically.

The unchanged legacy opt-in Realtime REST regression returned HTTP 404 for
`POST /v1/realtime/sessions` (“Invalid URL”), reproduced with the original
v0.4.1 request and the same configured key. The modern client-secret operation
preceding it and the existing Realtime WebSocket target checks succeeded. This
external baseline limitation is not reported as a passing live regression and
does not change or remove that existing test/API.

Mixed Live/Realtime guides describe both products; only the Live sections apply.
The private experimental adapter's older names/turn events are not public Live
compatibility aliases.
