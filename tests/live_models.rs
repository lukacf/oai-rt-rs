use oai_rt_rs::live::*;
use serde_json::{Value, json};

#[test]
fn frontend_permission_names_and_list_sizes_follow_complete_schema() {
    for name in ["error", "info", "session.input_audio.mute", "custom.9_-"] {
        let config = ClientConfig {
            data_channel: DataChannelConfig {
                allowed_client_events: Some(EventPermissions::Selected(vec![name.into()])),
                ..DataChannelConfig::default()
            },
        };
        config.validate().unwrap();
    }

    for name in [
        "",
        "all",
        "Session.start",
        ".x",
        "x.",
        "x..y",
        "foo.ä",
        "foo.\n",
    ] {
        let config = ClientConfig {
            data_channel: DataChannelConfig {
                allowed_client_events: Some(EventPermissions::Selected(vec![name.into()])),
                ..DataChannelConfig::default()
            },
        };
        assert!(config.validate().is_err(), "{name:?}");
    }
    for size in [256, 257] {
        let config = ClientConfig {
            data_channel: DataChannelConfig {
                allowed_client_events: Some(EventPermissions::Selected(vec![
                    "session.close".into();
                    size
                ])),
                allowed_server_events: Some(EventPermissions::Selected(vec![
                    ServerEventSelector {
                        event_type: "session.closed".into(),
                        response_event: None
                    };
                    size
                ])),
            },
        };
        assert_eq!(config.validate().is_ok(), size == 256);
    }
}

#[test]
fn named_tool_choices_follow_identifier_patterns_and_payload_debug_is_redacted() {
    for name in ["tool_1", "A-B", "0"] {
        ResponsesOptions {
            tool_choice: Some(ToolChoice::Named(NamedToolChoice::Function {
                name: name.into(),
            })),
            ..ResponsesOptions::default()
        }
        .validate()
        .unwrap();
    }
    for name in ["tool.name", "name space", "å"] {
        assert!(
            ResponsesOptions {
                tool_choice: Some(ToolChoice::Named(NamedToolChoice::Function {
                    name: name.into()
                })),
                ..ResponsesOptions::default()
            }
            .validate()
            .is_err()
        );
    }
    let config = SessionConfig {
        instructions: Field::Value("synthetic-private-payload".into()),
        ..SessionConfig::default()
    };
    assert!(!format!("{config:?}").contains("synthetic-private-payload"));
    assert!(!format!("{:?}", config.instructions).contains("synthetic-private-payload"));
}

#[test]
fn startup_representation_is_separate_from_realtime_and_private_live() {
    assert_eq!(
        serde_json::to_value(SessionConfig::default()).unwrap(),
        json!({"model":"gpt-live-1"})
    );
    let full = json!({
        "model":"gpt-live-1",
        "audio":{"format":{"type":"audio/pcm","rate":16000},"output":{"voice":{"id":"authorized"}}},
        "client":{"data_channel":{"allowed_client_events":[],"allowed_server_events":[{"type":"response.event","response_event":"response.completed"}]}},
        "delegation":{"type":"responses","responses":{
            "model":"gpt-5.5","instructions":null,"max_output_tokens":64,
            "parallel_tool_calls":true,"reasoning":{"effort":"low","summary":"auto"},
            "service_tier":"priority","text":{"verbosity":"low"},
            "tools":[{"type":"function","name":"sum","parameters":{},"strict":true},{"type":"web_search"}],
            "tool_choice":{"type":"function","name":"sum"}
        }},
        "input":[{"role":"user","content":[{"type":"input_text","text":"Hello"}]}],
        "instructions":"Be brief.","store":false
    });
    let config: SessionConfig = serde_json::from_value(full.clone()).unwrap();
    assert_eq!(serde_json::to_value(config).unwrap(), full);
}

#[test]
fn sparse_update_keeps_absent_null_and_value_distinct() {
    for value in [
        json!({}),
        json!({"delegation":null}),
        json!({"delegation":{"type":"client"}}),
        json!({"delegation":{"type":"responses"}}),
        json!({"delegation":{"type":"responses","responses":{}}}),
        json!({"delegation":{"type":"responses","responses":{"instructions":null,"tools":[]}}}),
    ] {
        let update: SessionUpdate = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(update).unwrap(), value);
    }
    let omitted: SessionUpdate = serde_json::from_value(json!({})).unwrap();
    let null: SessionUpdate = serde_json::from_value(json!({"delegation":null})).unwrap();
    assert_ne!(omitted, null);
}

#[test]
fn optional_nonnullable_fields_reject_null() {
    for field in ["audio", "client", "input", "store"] {
        let mut v = json!({"model":"gpt-live-1"});
        v[field] = Value::Null;
        assert!(
            serde_json::from_value::<SessionConfig>(v).is_err(),
            "{field}"
        );
    }
    for field in ["tools", "tool_choice"] {
        let mut v = json!({});
        v[field] = Value::Null;
        assert!(
            serde_json::from_value::<ResponsesOptions>(v).is_err(),
            "{field}"
        );
    }
}

#[test]
fn all_voices_audio_formats_and_backend_options_are_representable() {
    assert_eq!(VOICES.len(), 22);
    for voice in VOICES {
        let v: Voice = serde_json::from_value(json!(voice)).unwrap();
        assert_eq!(serde_json::to_value(v).unwrap(), json!(voice));
    }
    for format in [
        json!({"type":"audio/pcm","rate":24000}),
        json!({"type":"audio/pcm","rate":16000}),
        json!({"type":"audio/pcmu","rate":8000}),
        json!({"type":"audio/pcma","rate":8000}),
    ] {
        let v: AudioFormat = serde_json::from_value(format.clone()).unwrap();
        assert_eq!(serde_json::to_value(v).unwrap(), format);
    }
    for tier in [
        "auto",
        "default",
        "fast_tier_temp_pilot",
        "flex",
        "priority",
        "ultrafast",
    ] {
        serde_json::from_value::<ServiceTier>(json!(tier)).unwrap();
    }
    for effort in ["none", "minimal", "low", "medium", "high", "xhigh"] {
        serde_json::from_value::<ReasoningEffort>(json!(effort)).unwrap();
    }
    assert!(serde_json::from_value::<InitialRole>(json!("system")).is_err());
    assert!(
        serde_json::from_value::<Tool>(json!({"type":"mcp","server_url":"https://example.com"}))
            .is_err()
    );
}
