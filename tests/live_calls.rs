use oai_rt_rs::live::*;
use serde_json::json;

#[test]
fn sip_accept_type_is_not_a_primary_or_webrtc_session_field() {
    let accept = AcceptRequest {
        session: SipAcceptSession::new(SessionConfig::default()),
    };
    let wire = json!({"session":{"type":"live","model":"gpt-live-1"}});
    assert_eq!(serde_json::to_value(&accept).unwrap(), wire);
    decode_request::<AcceptRequest>(&wire.to_string())
        .unwrap()
        .validate()
        .unwrap();
    assert!(
        Codec::default()
            .decode_client(
                r#"{"type":"session.start","session":{"type":"live","model":"gpt-live-1"}}"#
            )
            .is_err()
    );
    let invalid = AcceptRequest {
        session: SipAcceptSession::new(SessionConfig {
            audio: Some(AudioConfig {
                format: Some(AudioFormat::default()),
                output: None,
            }),
            ..SessionConfig::default()
        }),
    };
    assert!(invalid.validate().is_err());
    for code in [299, 700] {
        assert!(RejectRequest { status_code: code }.validate().is_err());
    }
    for code in [300, 486, 699] {
        RejectRequest { status_code: code }.validate().unwrap();
    }
    for value in [
        json!({"status_code":300.5}),
        json!({"status_code":-1}),
        json!({}),
    ] {
        assert!(serde_json::from_value::<RejectRequest>(value).is_err());
    }
    ReferRequest {
        target_uri: "sip:agent@example.com".into(),
    }
    .validate()
    .unwrap();
    for uri in ["", " \t\n"] {
        assert!(
            ReferRequest {
                target_uri: uri.into()
            }
            .validate()
            .is_err()
        );
    }
}

#[test]
fn incoming_webhooks_keep_untrusted_headers_and_legacy_shape_distinct() {
    let codec = Codec::default();
    for kind in ["live.transport.incoming", "live.call.incoming"] {
        let mut raw = json!({"object":"event","type":kind,"id":"evt","created_at":1.25,"data":{
            "session_id":"opaque","sip_headers":[{"name":"From","value":"untrusted caller"}]
        },"future":true});
        if kind == "live.transport.incoming" {
            raw["data"]["type"] = json!("sip");
        }
        let frame = codec.decode_webhook(&raw.to_string()).unwrap();
        assert_eq!(frame.raw, raw);
        assert!(!format!("{frame:?}").contains("untrusted caller"));
        assert!(!matches!(frame.event, IncomingWebhookEvent::Unknown));
    }
    assert!(codec.decode_webhook(r#"{"type":"live.transport.incoming","id":"e","created_at":1,"data":{"session_id":"s","sip_headers":[]}}"#).is_err());
    assert!(codec.decode_webhook(r#"{"type":"live.transport.incoming","id":"e","id":"duplicate","created_at":1,"data":{}}"#).is_err());
    let future = codec
        .decode_webhook(r#"{"type":"live.future","payload":{"keep":true}}"#)
        .unwrap();
    assert!(matches!(future.event, IncomingWebhookEvent::Unknown));
    assert_eq!(future.raw["payload"]["keep"], true);
    assert!(codec.decode_webhook(r#"{"type":"live.call.incoming","id":"e","created_at":1,"object":null,"data":{"session_id":"s","sip_headers":[]}}"#).is_err());
}

#[test]
fn provider_error_code_is_required_but_nullable_and_dtmf_is_notification_only() {
    let codec = Codec::default();
    let unscoped = codec.decode_server(r#"{"type":"error","event_id":"e","error":{"code":"invalid_request_error","type":"invalid_request_error","message":"synthetic","param":null}}"#).unwrap();
    assert!(matches!(unscoped.event,ServerEvent::Error {error,..} if error.param.is_none()));
    assert_eq!(unscoped.raw["error"]["param"], serde_json::Value::Null);
    let frame = codec.decode_server(r#"{"type":"error","event_id":"e","error":{"code":null,"type":"invalid_request_error","message":"synthetic"}}"#).unwrap();
    assert!(matches!(frame.event,ServerEvent::Error {error,..} if error.code.is_none()));
    assert!(codec.decode_server(r#"{"type":"error","event_id":"e","error":{"type":"invalid_request_error","message":"missing code"}}"#).is_err());
    for key in ["0", "1", "9", "*", "#", "A", "B", "C", "D"] {
        for kind in ["transport.dtmf.received", "transport.dtmf.send"] {
            let wire = json!({"type":kind,"event_id":"e","event":key});
            codec.decode_server(&wire.to_string()).unwrap();
            assert!(codec.decode_client(&wire.to_string()).is_err());
        }
    }
    for key in ["", "11", "X", "é"] {
        assert!(
            codec
                .decode_server(
                    &json!({"type":"transport.dtmf.received","event_id":"e","event":key})
                        .to_string()
                )
                .is_err()
        );
    }
}

#[test]
fn backend_token_counts_use_provider_required_integers_not_float_timestamps() {
    for value in [json!(16.0), json!(16.5), json!(-1), json!("16")] {
        assert!(
            serde_json::from_value::<ResponsesOptions>(json!({"max_output_tokens":value})).is_err()
        );
    }
    let config: ResponsesOptions = serde_json::from_value(json!({"max_output_tokens":16})).unwrap();
    config.validate().unwrap();
    assert_eq!(
        serde_json::to_value(config).unwrap(),
        json!({"max_output_tokens":16})
    );
}
