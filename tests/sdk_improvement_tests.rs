use oai_rt_rs::VoiceEvent;

#[tokio::test]
async fn test_new_voice_events_mapping() {
    let _ = VoiceEvent::UserTranscriptDone {
        item_id: "item_1".to_string(),
        content_index: 0,
        transcript: "hello".to_string(),
    };
    let _ = VoiceEvent::ResponseCancelled {
        response_id: "resp_1".to_string(),
    };
}

#[tokio::test]
async fn test_session_state_methods() {
    // verify compilation of new methods would go here
}
