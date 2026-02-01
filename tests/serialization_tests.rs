use oai_rt_rs::protocol::client_events::ClientEvent;
use oai_rt_rs::protocol::models::{InputItem, AudioFormat, OutputModalities, Role, ConversationMode, MaxTokens, Infinite, ItemStatus, Session, SessionConfig, SessionKind, ResponseStatus};
use oai_rt_rs::protocol::server_events::ServerEvent;
use serde_json::json;

#[test]
fn test_session_update_deserialization() {
    let json = json!({
        "type": "session.update",
        "session": {
            "instructions": "You are a helpful assistant.",
            "turn_detection": {
                "type": "server_vad",
                "threshold": 0.5,
                "create_response": true
            },
            "output_modalities": ["audio"],
            "audio": {
                "input": {
                    "format": { "type": "audio/pcm", "rate": 24000 }
                }
            }
        }
    });

    let event: ClientEvent = serde_json::from_value(json).expect("Failed to deserialize session.update");
    match event {
        ClientEvent::SessionUpdate { session, .. } => {
            let session = session.as_ref();
            assert_eq!(session.config.output_modalities, Some(OutputModalities::Audio));
            
            // Check nested audio config
            if let Some(audio) = &session.config.audio {
                if let Some(input) = &audio.input {
                    if let Some(AudioFormat::Pcm { .. }) = &input.format {
                         // Correct variant
                    } else {
                        panic!("Missing or wrong format: {:?}", input.format);
                    }
                } else {
                    panic!("Missing input");
                }
            } else {
                panic!("Missing audio config");
            }
        }
        _ => panic!("Wrong event type"),
    }
}

#[test]
fn test_response_create_with_input_and_metadata() {
    let json = json!({
        "type": "response.create",
        "response": {
            "conversation": "none",
            "metadata": { "topic": "pizza" },
            "input": [
                {
                    "type": "item_reference",
                    "id": "item_ref_1"
                },
                {
                    "type": "message",
                    "role": "user",
                    "content": [
                        { "type": "input_text", "text": "Pineapple?" }
                    ]
                }
            ]
        }
    });

    let event: ClientEvent = serde_json::from_value(json).expect("Failed to deserialize response.create");
    match event {
        ClientEvent::ResponseCreate { response, .. } => {
            let config = response.expect("Missing response config");
            let config = config.as_ref();
            assert_eq!(config.conversation, Some(ConversationMode::None));
            
            let metadata = config.metadata.as_ref().expect("Missing metadata");
            assert_eq!(
                metadata.get("topic").and_then(|s| s.as_str()),
                Some("pizza")
            );

            let input = config.input.as_ref().expect("Missing input");
            assert_eq!(input.len(), 2);
            
            match &input[0] {
                InputItem::ItemReference { id } => assert_eq!(id, "item_ref_1"),
                InputItem::Message { .. } => panic!("Wrong item type at index 0"),
            }

            match &input[1] {
                InputItem::Message { role, .. } => assert!(matches!(role, Role::User)),
                InputItem::ItemReference { .. } => panic!("Wrong item type at index 1"),
            }
        }
        _ => panic!("Wrong event type"),
    }
}

#[test]
fn test_infinite_tokens_roundtrip() {
    let inf = MaxTokens::Infinite(Infinite::Inf);
    let serialized = serde_json::to_string(&inf).expect("Serialize inf");
    assert_eq!(serialized, "\"inf\"");
    
    let deserialized: MaxTokens = serde_json::from_str(&serialized).expect("Deserialize inf");
    assert!(matches!(deserialized, MaxTokens::Infinite(Infinite::Inf)));
}

#[test]
fn test_server_event_flat_deserialization() {
    let json = json!({
        "type": "response.output_text.delta",
        "event_id": "evt_1",
        "response_id": "resp_1",
        "item_id": "item_1",
        "output_index": 0,
        "content_index": 0,
        "delta": "hello"
    });
    
    let event: ServerEvent = serde_json::from_value(json).expect("Deserialize flat event");
    assert_eq!(event.event_id(), Some("evt_1"));
    match event {
        ServerEvent::ResponseOutputTextDelta { delta, .. } => assert_eq!(delta, "hello"),
        _ => panic!("Wrong variant"),
    }
}

#[test]
fn test_serialization_roundtrip() {
    let original = json!({
        "type": "conversation.item.create",
        "item": {
            "type": "message",
            "role": "user",
            "content": [
                {
                    "type": "input_text",
                    "text": "Hello world"
                }
            ]
        }
    });
    
    let event: ClientEvent = serde_json::from_value(original.clone()).expect("Deserialize in roundtrip");
    let serialized = serde_json::to_value(&event).expect("Serialize in roundtrip");
    
    assert_eq!(serialized.get("type"), original.get("type"));
}

#[test]
fn test_item_status_copy() {
    let s = ItemStatus::Completed;
    let _ = s; // Should be Copy
    assert_eq!(s, ItemStatus::Completed);
}

#[test]
fn test_session_struct_update() {
    let mut config = SessionConfig::new(
        SessionKind::Realtime,
        "gpt-realtime",
        OutputModalities::Audio,
    );
    config.instructions = Some("Test instructions".to_string());

    let session = Session {
        id: "sess_123".to_string(),
        object: "realtime.session".to_string(),
        expires_at: 123,
        config,
    };

    assert_eq!(session.config.model.as_str(), "gpt-realtime");
    assert_eq!(session.config.instructions.as_deref(), Some("Test instructions"));
    assert_eq!(session.config.output_modalities, OutputModalities::Audio);
}

#[test]
fn test_response_status_enum() {
    let json = json!("cancelled");
    let status: ResponseStatus = serde_json::from_value(json).unwrap();
    assert_eq!(status, ResponseStatus::Cancelled);
}
