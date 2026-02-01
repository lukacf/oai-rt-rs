Realtime transcription
======================

Learn how to transcribe audio in real-time with the Realtime API.

You can use the Realtime API for transcription-only use cases, either with input from a microphone or from a file. For example, you can use it to generate subtitles or transcripts in real-time. With the transcription-only mode, the model will not generate responses.

If you want the model to produce responses, you can use the Realtime API in [speech-to-speech conversation mode](/docs/guides/realtime-conversations).

Realtime transcription sessions
-------------------------------

To use the Realtime API for transcription, you need to create a transcription session, connecting via [WebSockets](/docs/guides/realtime?use-case=transcription#connect-with-websockets) or [WebRTC](/docs/guides/realtime?use-case=transcription#connect-with-webrtc).

Unlike the regular Realtime API sessions for conversations, the transcription sessions typically don't contain responses from the model.

The transcription session object uses the same base session shape, but it always has a `type` of `"transcription"`:

```json
{
  "object": "realtime.session",
  "type": "transcription",
  "id": "session_abc123",
  "audio": {
    "input": {
      "format": {
        "type": "audio/pcm",
        "rate": 24000
      },
      "noise_reduction": {
        "type": "near_field"
      },
      "transcription": {
        "model": "gpt-4o-transcribe",
        "prompt": "",
        "language": "en"
      },
      "turn_detection": {
        "type": "server_vad",
        "threshold": 0.5,
        "prefix_padding_ms": 300,
        "silence_duration_ms": 500
      }
    }
  },
  "include": [
    "item.input_audio_transcription.logprobs"
  ]
}
```

### Session fields

*   `type`: Always `transcription` for realtime transcription sessions.
*   `audio.input.format`: Input encoding for audio that you append to the buffer. Supported types are:
    *   `audio/pcm` (24 kHz mono PCM; only a `rate` of `24000` is supported).
    *   `audio/pcmu` (G.711 μ-law).
    *   `audio/pcma` (G.711 A-law).
*   `audio.input.noise_reduction`: Optional noise reduction that runs before VAD and turn detection. Use `{ "type": "near_field" }`, `{ "type": "far_field" }`, or `null` to disable.
*   `audio.input.transcription`: Optional asynchronous transcription of input audio. Supply:
    *   `model`: One of `whisper-1`, `gpt-4o-transcribe-latest`, `gpt-4o-mini-transcribe`, or `gpt-4o-transcribe`.
    *   `language`: ISO-639-1 code such as `en`.
    *   `prompt`: Prompt text or keyword list (model-dependent) that guides the transcription output.
*   `audio.input.turn_detection`: Optional automatic voice activity detection (VAD). Set to `null` to manage turn boundaries manually. For `server_vad`, you can tune `threshold`, `prefix_padding_ms`, `silence_duration_ms`, `interrupt_response`, `create_response`, and `idle_timeout_ms`. For `semantic_vad`, configure `eagerness`, `interrupt_response`, and `create_response`.
*   `include`: Optional list of additional fields to stream back on events (for example `item.input_audio_transcription.logprobs`).

You can find more information about the transcription session object in the [API reference](/docs/api-reference/realtime-sessions/transcription_session_object).

Handling transcriptions
-----------------------

When using the Realtime API for transcription, you can listen for the `conversation.item.input_audio_transcription.delta` and `conversation.item.input_audio_transcription.completed` events.

For `whisper-1` the `delta` event will contain full turn transcript, same as `completed` event. For `gpt-4o-transcribe` and `gpt-4o-mini-transcribe` the `delta` event will contain incremental transcripts as they are streamed out from the model.

Here is an example transcription delta event:

```json
{
  "event_id": "event_2122",
  "type": "conversation.item.input_audio_transcription.delta",
  "item_id": "item_003",
  "content_index": 0,
  "delta": "Hello,"
}
```

Here is an example transcription completion event:

```json
{
  "event_id": "event_2122",
  "type": "conversation.item.input_audio_transcription.completed",
  "item_id": "item_003",
  "content_index": 0,
  "transcript": "Hello, how are you?"
}
```

Note that ordering between completion events from different speech turns is not guaranteed. You should use `item_id` to match these events to the `input_audio_buffer.committed` events and use `input_audio_buffer.committed.previous_item_id` to handle the ordering.

To send audio data to the transcription session, you can use the `input_audio_buffer.append` event.

You have 2 options:

*   Use a streaming microphone input
*   Stream data from a wav file

Voice activity detection
------------------------

The Realtime API supports automatic voice activity detection (VAD). Enabled by default, VAD will control when the input audio buffer is committed, therefore when transcription begins.

Read more about configuring VAD in our [Voice Activity Detection](/docs/guides/realtime-vad) guide.

You can also disable VAD by setting the `audio.input.turn_detection` property to `null`, and control when to commit the input audio on your end.

Additional configurations
-------------------------

### Noise reduction

Use the `audio.input.noise_reduction` property to configure how to handle noise reduction in the audio stream.

*   `{ "type": "near_field" }`: Use near-field noise reduction (default).
*   `{ "type": "far_field" }`: Use far-field noise reduction.
*   `null`: Disable noise reduction.

### Using logprobs

You can use the `include` property to include logprobs in the transcription events, using `item.input_audio_transcription.logprobs`.

Those logprobs can be used to calculate the confidence score of the transcription.

```json
{
  "type": "session.update",
  "session": {
    "audio": {
      "input": {
        "format": {
          "type": "audio/pcm",
          "rate": 24000
        },
        "transcription": {
          "model": "gpt-4o-transcribe"
        },
        "turn_detection": {
          "type": "server_vad",
          "threshold": 0.5,
          "prefix_padding_ms": 300,
          "silence_duration_ms": 500
        }
      }
    },
    "include": [
      "item.input_audio_transcription.logprobs"
    ]
  }
}
```
