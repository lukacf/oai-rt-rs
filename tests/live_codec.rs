use oai_rt_rs::live::*;
use serde_json::json;

#[test]
fn every_base_command_has_exact_public_wire_shape() {
    let codec = Codec::default();
    for wire in [
        json!({"type":"session.start","session":{"model":"gpt-live-1"}}),
        json!({"type":"session.update","session":{}}),
        json!({"type":"session.close","event_id":null}),
        json!({"type":"session.input_audio.mute"}),
        json!({"type":"session.input_audio.unmute"}),
        json!({"type":"session.input_audio.append","audio":"AAA="}),
        json!({"type":"session.instructions.append","content":"quiet","delegation_id":null}),
        json!({"type":"session.thinking.append","content":"facts","delegation_id":"opaque"}),
        json!({"type":"session.commentary.append","content":"ready","delegation_id":null}),
    ] {
        let event = codec.decode_client(&wire.to_string()).unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&codec.encode(&event).unwrap()).unwrap(),
            wire
        );
        assert!(!format!("{event:?}").contains("quiet"));
    }
}

#[test]
fn outgoing_rejects_extras_collisions_nulls_and_old_protocol() {
    let codec = Codec::default();
    for wire in [
        r#"{"type":"session.close","model":"gpt-live-1"}"#,
        r#"{"type":"session.close","type":"session.close"}"#,
        r#"{"type":"session.start","session":{"model":"gpt-live-1","type":"live"}}"#,
        r#"{"type":"session.start","session":{"model":"gpt-live-1","audio":{"output":{"voice":{"id":"v","extra":true}}}}}"#,
        r#"{"type":"session.start","session":{"model":"gpt-live-1","delegation":{"type":"responses","responses":{"model":"gpt-5.5","temperature":0.2}}}}"#,
        r#"{"type":"session.update","session":{"audio":{"format":{"type":"audio/pcm","rate":24000}}}}"#,
        r#"{"type":"session.thinking.append","content":"missing nullable"}"#,
        r#"{"type":"input_audio_buffer.commit"}"#,
        r#"{"type":"conversation.item.create","item":{}}"#,
        r#"{"type":"response.cancel"}"#,
    ] {
        assert!(codec.decode_client(wire).is_err(), "{wire}");
    }
    let max = ClientEvent {
        event_id: Field::Value("é".repeat(512)),
        command: Command::Close,
    };
    assert!(codec.encode(&max).is_ok());
    let over = ClientEvent {
        event_id: Field::Value("a".repeat(513)),
        command: Command::Close,
    };
    assert!(codec.encode(&over).is_err());
}

#[test]
fn raw_audio_rejects_container_and_half_samples_without_converting_codecs() {
    assert_eq!(
        decode_audio("AAA=", AudioFormat::default()).unwrap(),
        [0, 0]
    );
    assert!(decode_audio("AA==", AudioFormat::default()).is_err());
    assert_eq!(
        decode_audio("AA==", AudioFormat::Pcmu { rate: 8000 }).unwrap(),
        [0]
    );
    assert!(decode_audio("not base64", AudioFormat::default()).is_err());
    assert!(validate_audio_bytes(b"RIFF0000WAVE", AudioFormat::default()).is_err());
    assert!(AudioFormat::Pcm { rate: 8000 }.validate().is_err());
    assert!(AudioFormat::Pcma { rate: 24000 }.validate().is_err());
}

#[test]
fn inbound_fractional_timing_extensions_and_terminal_status_survive() {
    let codec = Codec::default();
    let transcript = r#"{"type":"session.output_transcript.delta","event_id":"e","delta":" hello ","start_ms":0.25,"end_ms":1.75,"future":true}"#;
    let event = codec.decode_server(transcript).unwrap();
    assert_eq!(event.raw["future"], true);
    assert!(
        matches!(event.event, ServerEvent::OutputTranscriptDelta { delta, start_ms:0.25,end_ms:1.75,.. } if delta == " hello ")
    );
    let event = codec
        .decode_server(r#"{"type":"future.event","data":{"value":1}}"#)
        .unwrap();
    assert!(matches!(event.event, ServerEvent::Unknown));
    assert_eq!(event.raw["data"]["value"], 1);
    for bad in [
        r#"{"type":"session.output_transcript.delta","event_id":"e","delta":"x","start_ms":0}"#,
        r#"{"type":"session.input_audio.muted","event_id":"e","client_event_id":null}"#,
        r#"{"type":"session.output_audio.delta","delta":"AAA=","start_ms":null}"#,
        r#"{"type":"session.usage.updated","event_id":"e","usage":null}"#,
        r#"{"type":"session.usage.updated","event_id":"e","usage":{"seconds":1,"seconds":2}}"#,
    ] {
        assert!(codec.decode_server(bad).is_err(), "{bad}");
    }
    let frame = codec.decode_server(r#"{"type":"session.closed","event_id":"e","reason":"close_requested","session":{"id":"s","model":"gpt-live-1","status":"active","expires_at":1788307200.5},"usage":{"seconds":3.5}}"#).unwrap();
    assert!(matches!(
        frame.event,
        ServerEvent::Closed {
            usage: Usage { seconds: 3.5 },
            ..
        }
    ));
}

#[test]
fn all_server_event_families_decode() {
    let session = json!({"id":"s","model":"gpt-live-1","status":"active","expires_at":10});
    let mut wires = vec![
        json!({"type":"session.started","event_id":"e","session":session}),
        json!({"type":"session.updated","event_id":"e","session":session}),
        json!({"type":"session.output_audio.delta","delta":"AAA="}),
        json!({"type":"session.output_audio.delta","delta":"AAA=","start_ms":0.1,"end_ms":0.2}),
        json!({"type":"session.input_audio.append","audio":"AAA="}),
        json!({"type":"session.delegation.created","event_id":"e","offset_ms":0.5,"delegation":{"id":"opaque","type":"delegation","target":"client"}}),
        json!({"type":"session.delegation.created","event_id":"e","offset_ms":0.5,"delegation":{"id":"opaque","type":"delegation","target":"responses","response_id":"r"}}),
        json!({"type":"response.event","event_id":"e","event":{"type":"response.created","response":{"id":"r","output":[]}}}),
        json!({"type":"session.usage.updated","event_id":"e","usage":{"seconds":0.5},"context_window":{"usage_ratio":0.95}}),
        json!({"type":"error","event_id":"e","error":{"code":"bad","message":"private","type":"invalid_request_error","client_event_id":"request"}}),
        json!({"type":"info","event_id":"e","code":"notice","message":"private"}),
        json!({"type":"transport.failed","event_id":"e","session_id":"s","error":{"code":"bad","message":"private","type":"call_error"}}),
    ];
    for kind in ["session.input_audio.muted", "session.input_audio.unmuted"] {
        wires.push(json!({"type":kind,"event_id":"e","client_event_id":"c"}));
    }

    for kind in [
        "session.instructions.appended",
        "session.thinking.appended",
        "session.commentary.appended",
    ] {
        wires.push(json!({"type":kind,"event_id":"e","start_ms":1.5,"end_ms":1.5}));
    }
    for kind in [
        "session.input_transcript.delta",
        "session.output_transcript.delta",
    ] {
        wires.push(json!({"type":kind,"event_id":"e","start_ms":1.5,"end_ms":2.5,"delta":" x "}));
    }
    for kind in ["transport.dtmf.received", "transport.dtmf.send"] {
        wires.push(json!({"type":kind,"event_id":"e","event":"1"}));
    }
    for kind in ["transport.ringing", "transport.answered"] {
        wires.push(json!({"type":kind,"event_id":"e","session_id":"s"}));
    }
    for wire in wires {
        let frame = Codec::default().decode_server(&wire.to_string()).unwrap();
        assert!(!matches!(frame.event, ServerEvent::Unknown), "{wire}");
        assert_eq!(frame.raw, wire);
        assert!(!format!("{frame:?}").contains("private"));
    }
}

#[test]
fn audio_consumption_distinguishes_primary_and_sideband_formats() {
    let codec = Codec::default();
    let primary = codec
        .decode_server(r#"{"type":"session.output_audio.delta","delta":"AA=="}"#)
        .unwrap();
    let chunk = primary
        .audio(ConnectionRole::Primary, AudioFormat::Pcma { rate: 8000 })
        .unwrap()
        .unwrap();
    assert_eq!(chunk.bytes, [0]);
    assert_eq!(chunk.interval, None);
    assert!(
        primary
            .audio(ConnectionRole::Sideband, AudioFormat::Pcma { rate: 8000 })
            .is_err()
    );
    let sideband = codec
        .decode_server(
            r#"{"type":"session.output_audio.delta","delta":"AAA=","start_ms":0.25,"end_ms":0.5}"#,
        )
        .unwrap();
    let chunk = sideband
        .audio(ConnectionRole::Sideband, AudioFormat::Pcma { rate: 8000 })
        .unwrap()
        .unwrap();
    assert_eq!(chunk.format, AudioFormat::Pcm { rate: 24000 });
    assert_eq!(
        chunk.interval,
        Some(AudioInterval {
            start_ms: 0.25,
            end_ms: 0.5
        })
    );
    let reflected = codec
        .decode_server(r#"{"type":"session.input_audio.append","audio":"AAA="}"#)
        .unwrap();
    assert_eq!(
        reflected
            .audio(ConnectionRole::Sideband, AudioFormat::default())
            .unwrap()
            .unwrap()
            .source,
        AudioSource::ReflectedInput
    );
    assert!(
        reflected
            .audio(ConnectionRole::Primary, AudioFormat::default())
            .unwrap()
            .is_none()
    );
}

#[test]
fn structural_limits_are_faithful_without_a_fake_tokenizer() {
    let mut config = SessionConfig::default();
    let input = InitialItem {
        role: InitialRole::User,
        content: vec![InitialText {
            text: "test".into(),
            text_type: Some(InitialTextType::InputText),
        }],
        id: Field::Absent,
        status: Field::Null,
        item_type: None,
    };
    config.input = Some(vec![input.clone(); 128]);
    config.validate().unwrap();
    config.input.as_mut().unwrap().push(input.clone());
    assert!(config.validate().is_err());
    config.input = Some(vec![InitialItem {
        content: vec![],
        ..input.clone()
    }]);
    assert!(config.validate().is_err());
    config.input = Some(vec![InitialItem {
        role: InitialRole::Assistant,
        ..input
    }]);
    assert!(config.validate().is_err());
    for limit in [0, 15] {
        assert!(
            ResponsesOptions {
                max_output_tokens: Field::Value(limit),
                ..ResponsesOptions::default()
            }
            .validate()
            .is_err()
        );
    }
    ResponsesOptions {
        max_output_tokens: Field::Value(16),
        ..ResponsesOptions::default()
    }
    .validate()
    .unwrap();
    ClientEvent::new(Command::ThinkingAppend {
        content: "word ".repeat(600),
        delegation_id: Nullable(None),
    })
    .validate()
    .unwrap();
}

#[test]
fn response_event_permissions_require_a_nested_selector_in_both_directions() {
    let codec = Codec::default();
    for selector in [
        json!({"type":"response.event"}),
        json!({"type":"session.started","response_event":"response.completed"}),
    ] {
        let wire = json!({"type":"session.start","session":{
            "model":"gpt-live-1","client":{"data_channel":{"allowed_server_events":[selector]}}
        }});
        assert!(codec.decode_client(&wire.to_string()).is_err());
    }
    let valid = json!({"type":"session.start","session":{
        "model":"gpt-live-1","client":{"data_channel":{"allowed_server_events":[
            {"type":"response.event","response_event":"response.completed"},{"type":"session.started"}
        ]}}
    }});
    codec.decode_client(&valid.to_string()).unwrap();
}
