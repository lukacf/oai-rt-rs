#![cfg(feature = "experimental-gpt-live")]

use oai_rt_rs::experimental::gpt_live::{
    CreateCallRequest, Direction, GptLiveCredentials, SidebandHeaders, TerminalClass, WireSummary,
    decode_server_event,
};
use serde_json::json;
use std::io::{self, Write};
use std::sync::{Arc, Mutex};
use tracing_subscriber::fmt::MakeWriter;

#[derive(Clone, Default)]
struct SharedWriter(Arc<Mutex<Vec<u8>>>);

struct SharedGuard(Arc<Mutex<Vec<u8>>>);

impl Write for SharedGuard {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .expect("writer lock")
            .extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for SharedWriter {
    type Writer = SharedGuard;

    fn make_writer(&'a self) -> Self::Writer {
        SharedGuard(Arc::clone(&self.0))
    }
}

#[test]
fn diagnostics_and_debug_output_never_contain_private_payload_material() {
    let writer = SharedWriter::default();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .without_time()
        .with_max_level(tracing::Level::DEBUG)
        .with_writer(writer.clone())
        .finish();

    tracing::subscriber::with_default(subscriber, || {
        WireSummary::event(Direction::ToOpenAi, "delegation.context.append", 8192).emit();
        WireSummary::terminal(
            Direction::FromOpenAi,
            "session.started",
            4096,
            TerminalClass::Codec,
        )
        .emit();
        WireSummary::event(
            Direction::FromOpenAi,
            "unsafe\nFIXTURE_PRIVATE_UNKNOWN_SECRET",
            32,
        )
        .emit();
    });

    let credentials = GptLiveCredentials::new(
        "FIXTURE_BEARER_TOKEN",
        SidebandHeaders {
            account_id: Some("FIXTURE_ACCOUNT_ID".to_owned()),
            attestation: Some("FIXTURE_ATTESTATION".to_owned()),
            session_id: Some("FIXTURE_SESSION_ID".to_owned()),
            thread_id: Some("FIXTURE_THREAD_ID".to_owned()),
            x_session_id: Some("FIXTURE_X_SESSION_ID".to_owned()),
            ..SidebandHeaders::default()
        },
    );
    let request: CreateCallRequest = serde_json::from_value(json!({
        "sdp": "v=0\r\nFIXTURE_PRIVATE_OFFER_SDP",
        "session": {
            "model": "gpt-live-1-codex",
            "audio": { "output": { "voice": "cove" } },
            "instructions": "FIXTURE_PRIVATE_INSTRUCTIONS"
        }
    }))
    .expect("private request fixture");
    let transcript = decode_server_event(include_str!(
        "fixtures/gpt_live_v3/input_transcript_added.json"
    ))
    .expect("transcript fixture");
    let turn = decode_server_event(include_str!("fixtures/gpt_live_v3/turn_done.json"))
        .expect("turn fixture");
    let delegation =
        decode_server_event(include_str!("fixtures/gpt_live_v3/delegation_created.json"))
            .expect("delegation fixture");
    let unknown = decode_server_event(
        r#"{"type":"unsafe.FIXTURE_PRIVATE_UNKNOWN_KIND","secret":"FIXTURE_PRIVATE_UNKNOWN_SECRET","provider_id":"rtc_fixture_private_id","audio":"RklYVFVSRV9QUklWQVRFX0FVRElP"}"#,
    )
    .expect("unknown fixture");
    let output = format!(
        "{}\n{credentials:?}\n{request:?}\n{transcript:?}\n{turn:?}\n{delegation:?}\n{unknown:?}",
        String::from_utf8(writer.0.lock().expect("writer lock").clone()).expect("UTF-8 log")
    );

    assert!(output.contains("delegation.context.append"));
    assert!(output.contains("byte_count=8192"));
    assert!(output.contains("local_correlation"));
    assert!(output.contains("terminal_class=\"codec\""));
    for secret in [
        "FIXTURE_BEARER_TOKEN",
        "FIXTURE_ACCOUNT_ID",
        "FIXTURE_ATTESTATION",
        "FIXTURE_SESSION_ID",
        "FIXTURE_THREAD_ID",
        "FIXTURE_X_SESSION_ID",
        "FIXTURE_PRIVATE_INPUT_TRANSCRIPT",
        "FIXTURE_PRIVATE_INSTRUCTIONS",
        "FIXTURE_PRIVATE_UNKNOWN_KIND",
        "v=0",
        "rtc_fixture_private_id",
        "item_fixture_delegation",
        "turn_fixture_assistant",
        "handoff_fixture_private",
        "RklYVFVSRV9QUklWQVRFX0FVRElP",
        "FIXTURE_PRIVATE_UNKNOWN_SECRET",
    ] {
        assert!(!output.contains(secret), "diagnostics leaked {secret}");
    }
}
