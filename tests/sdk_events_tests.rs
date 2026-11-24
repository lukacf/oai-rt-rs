use oai_rt_rs::protocol::server_events::ServerEvent;
use oai_rt_rs::sdk::events::SdkEvent;
use oai_rt_rs::{ApiErrorType, ServerError};

#[test]
fn transcription_completion_preserves_billing_and_language_detection() {
    use oai_rt_rs::TranscriptionUsage;
    use serde_json::json;

    for (usage, languages) in [
        (json!({"type":"duration", "seconds":2.5}), None),
        (
            json!({"type":"tokens", "input_tokens":4, "output_tokens":2, "total_tokens":6}),
            Some(json!([{"code":"en"}])),
        ),
        (
            json!({"input_tokens":4, "output_tokens":2, "total_tokens":6}),
            Some(json!([])),
        ),
    ] {
        let mut wire = json!({
            "type":"conversation.item.input_audio_transcription.completed",
            "event_id":"evt", "item_id":"i", "content_index":0, "transcript":"hello",
            "usage": usage
        });
        if let Some(languages) = &languages {
            wire["languages"] = languages.clone();
        }
        let raw: ServerEvent = serde_json::from_value(wire).unwrap();
        let Some(SdkEvent::InputTranscriptionCompleted {
            usage: Some(billing),
            languages: detected,
            ..
        }) = SdkEvent::from_server(raw)
        else {
            panic!("expected transcription completion");
        };
        assert_eq!(
            detected.map(|v| serde_json::to_value(v).unwrap()),
            languages
        );
        match billing {
            TranscriptionUsage::Duration { seconds } => {
                assert_eq!(Some(seconds), usage["seconds"].as_f64());
            }
            TranscriptionUsage::Tokens(counts) => assert_eq!(
                Some(u64::from(counts.total_tokens)),
                usage["total_tokens"].as_u64()
            ),
        }
    }
}

#[test]
fn sdk_event_maps_text_delta() {
    let evt = ServerEvent::ResponseOutputTextDelta {
        event_id: "evt_1".to_string(),
        response_id: "resp_1".to_string(),
        item_id: "item_1".to_string(),
        output_index: 0,
        content_index: 0,
        delta: "hi".to_string(),
    };

    let mapped = SdkEvent::from_server(evt).expect("event maps");
    match mapped {
        SdkEvent::TextDelta {
            response_id,
            item_id,
            delta,
            ..
        } => {
            assert_eq!(response_id, "resp_1");
            assert_eq!(item_id, "item_1");
            assert_eq!(delta, "hi");
        }
        other => panic!("unexpected mapping: {other:?}"),
    }
}

#[test]
fn server_error_marks_response_cancel_race_as_benign() {
    let error = ServerError {
        error_type: ApiErrorType::InvalidRequestError,
        code: Some("response_cancel_not_active".to_string()),
        message: "Cancellation failed: no active response found".to_string(),
        param: None,
        event_id: None,
    };

    assert!(error.is_response_cancel_not_active());
}
