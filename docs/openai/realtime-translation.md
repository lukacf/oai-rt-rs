Realtime translation
====================

Realtime translation uses a dedicated session architecture for live
interpretation.

Key differences from voice-agent sessions
-----------------------------------------

*   Connect WebSockets to `/v1/realtime/translations?model=gpt-realtime-translate`.
*   Connect WebRTC SDP offers to `/v1/realtime/translations` using a
    translation client secret from `/v1/realtime/translations/client_secrets`.
*   Configure the target language with `session.audio.output.language`.
*   Stream source audio continuously with `session.input_audio_buffer.append`.
*   Listen for `session.output_audio.delta`, `session.output_transcript.delta`,
    and `session.input_transcript.delta`.
*   Do not call `response.create`; translation has no assistant turn,
    response lifecycle, tool call, or conversation state to manage.

Example WebSocket flow
----------------------

```json
{
  "type": "session.update",
  "session": {
    "audio": {
      "output": {
        "language": "es"
      }
    }
  }
}
```

```json
{
  "type": "session.input_audio_buffer.append",
  "audio": "base64_pcm16"
}
```

Example output events:

```json
{ "type": "session.output_audio.delta", "delta": "base64_pcm16" }
{ "type": "session.output_transcript.delta", "delta": "Hola" }
{ "type": "session.input_transcript.delta", "delta": "Hello" }
```

Production notes
----------------

Use one translation session per target language. Keep speaker tracks separate
when translating multi-participant conversations. Include
`OpenAI-Safety-Identifier` on trusted server requests when the application maps
traffic to individual users.
