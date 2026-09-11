#[path = "../examples/support/mod.rs"]
mod support;

use futures::{SinkExt, StreamExt};
use oai_rt_rs::live::{AudioFormat, ClientOptions, Codec, LiveClient, ServerEvent, SessionConfig};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_tungstenite::{accept_async, tungstenite::Message};

#[test]
fn speech_rejects_silence_and_sparse_impulses_at_every_primary_rate() {
    for rate in [8_000, 16_000, 24_000] {
        for impulse in [0_i16, 1_000, i16::MAX, i16::MIN] {
            let pcm: Vec<u8> = (0..rate)
                .flat_map(|index| if index % (rate / 50) == 0 { impulse } else { 0 }.to_le_bytes())
                .collect();
            let mut speech = support::Speech::default();
            speech.add(&pcm, AudioFormat::Pcm { rate }).unwrap();
            assert!(
                !speech.qualified(),
                "isolated high peaks are not sustained speech"
            );
        }
        let pcm: Vec<u8> = (0..rate / 5)
            .flat_map(|_| 1_000_i16.to_le_bytes())
            .collect();
        let mut speech = support::Speech::default();
        for chunk in pcm.chunks(14) {
            speech.add(chunk, AudioFormat::Pcm { rate }).unwrap();
        }
        assert!(
            speech.qualified(),
            "windowing must survive arbitrary chunk boundaries"
        );
        assert_eq!(speech.report()["voiced_ms"], 200);
    }
}

#[test]
fn g711_decoding_and_duration_are_not_packet_or_peak_counts() {
    assert_eq!(support::decode_mulaw(0xff), 0);
    assert_eq!(support::decode_mulaw(0x80), 32124);
    assert_eq!(support::decode_mulaw(0x00), -32124);
    assert_eq!(support::decode_alaw(0xd5), 8);
    assert_eq!(support::decode_alaw(0x55), -8);
    assert_eq!(support::decode_alaw(0xaa), 32256);
    for (format, silence, signal) in [
        (AudioFormat::Pcmu { rate: 8000 }, 0xff, 0x80),
        (AudioFormat::Pcma { rate: 8000 }, 0xd5, 0xaa),
    ] {
        let mut speech = support::Speech::default();
        let impulses: Vec<_> = (0..8000)
            .map(|n| if n % 160 == 0 { signal } else { silence })
            .collect();
        speech.add(&impulses, format).unwrap();
        assert!(!speech.qualified());
        speech.add(&vec![signal; 1600], format).unwrap();
        assert!(speech.qualified());
        assert_eq!(speech.report()["voiced_ms"], 200);
    }
}

fn ack(kind: &str, id: &str) -> oai_rt_rs::live::ServerFrame {
    Codec::default()
        .decode_server(
            &json!({
                "type":kind,"event_id":"e","client_event_id":id,"start_ms":0,"end_ms":1
            })
            .to_string(),
        )
        .unwrap()
}

#[test]
fn ack_evidence_requires_exact_type_id_pairs_not_counts() {
    let mut acks = support::Acks::new(&[
        ("session.thinking.appended", "thinking-a"),
        ("session.thinking.appended", "thinking-b"),
        ("session.commentary.appended", "commentary"),
    ]);
    for id in ["unrelated-1", "unrelated-2", "unrelated-3"] {
        acks.observe(&ack("session.thinking.appended", id));
    }
    assert_eq!(acks.count(), 0);
    acks.observe(&ack("session.commentary.appended", "thinking-a"));
    assert_eq!(acks.count(), 0);
    for _ in 0..3 {
        acks.observe(&ack("session.thinking.appended", "thinking-a"));
    }
    assert_eq!(acks.count(), 1);
    assert!(!acks.complete());
    acks.observe(&ack("session.thinking.appended", "thinking-b"));
    acks.observe(&ack("session.commentary.appended", "commentary"));
    assert!(acks.complete());
}

#[test]
fn fork_and_attach_evidence_requires_actual_session_identity() {
    support::verify_identity(None, "created", "created").unwrap();
    support::verify_identity(Some("source"), "created", "created").unwrap();
    assert!(support::verify_identity(None, "created", "unrelated").is_err());
    assert!(support::verify_identity(Some("source"), "source", "source").is_err());
    assert!(support::verify_identity(Some("source"), "created", "source").is_err());
}

#[test]
fn only_explicit_exact_browser_denial_can_be_whitelisted() {
    let expected = json!({"type":"error","event_id":"e","error":{
        "type":"invalid_request_error","code":"event_not_allowed","message":"synthetic","client_event_id":"browser-restricted"
    }});
    let frame = Codec::default()
        .decode_server(&expected.to_string())
        .unwrap();
    assert!(support::check_frame(&frame).is_err());
    support::check_frame_for(&frame, true).unwrap();
    for field in ["type", "code", "client_event_id"] {
        let mut wire = expected.clone();
        wire["error"][field] = json!("unrelated");
        let frame = Codec::default().decode_server(&wire.to_string()).unwrap();
        assert!(support::check_frame_for(&frame, true).is_err());
    }
}

#[tokio::test]
async fn final_drain_retains_usage_but_never_greenwashes_provider_or_decode_errors() {
    for unexpected in [
        json!({"type":"error","event_id":"bad","error":{"type":"invalid_request_error","code":null,"message":"synthetic"}}),
        json!({"type":"session.output_transcript.delta","event_id":"bad","delta":"missing timing"}),
        json!({"type":"response.event","event_id":"bad","event":{"type":"response.completed"}}),
        Value::Null,
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = LiveClient::with_options(
            "synthetic",
            ClientOptions {
                base_url: format!("http://{}/v1/", listener.local_addr().unwrap())
                    .parse()
                    .unwrap(),
                request_timeout: Duration::from_secs(2),
                ..ClientOptions::default()
            },
        )
        .unwrap();
        let has_error = !unexpected.is_null();
        let server = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut ws = accept_async(tcp).await.unwrap();
            ws.next().await.unwrap().unwrap();
            let session = json!({"id":"s","model":"gpt-live-1","status":"active","expires_at":1});
            ws.send(Message::Text(
                json!({"type":"session.started","event_id":"start","session":session})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
            ws.next().await.unwrap().unwrap();
            if has_error {
                ws.send(Message::Text(unexpected.to_string().into()))
                    .await
                    .unwrap();
            }
            ws.send(Message::Text(
                json!({"type":"session.closed","event_id":"closed","session":session,
                "reason":"close_requested","usage":{"seconds":3}})
                .to_string()
                .into(),
            ))
            .await
            .unwrap();
        });
        let mut connection = client.connect(SessionConfig::default()).await.unwrap();
        let finalization = support::finalize(&mut connection).await;
        assert!(
            matches!(&finalization.closed, Ok(frame) if matches!(frame.event, ServerEvent::Closed { .. }))
        );
        assert_eq!(finalization.unexpected.is_some(), has_error);
        assert_eq!(finalization.finish().is_err(), has_error);
        server.await.unwrap();
    }
}
