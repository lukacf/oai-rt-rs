use oai_rt_rs::protocol::server_events::ServerEvent;
use oai_rt_rs::sdk::events::SdkEvent;
use oai_rt_rs::{ApiErrorType, ServerError};

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
