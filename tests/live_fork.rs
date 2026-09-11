use futures::{SinkExt, StreamExt};
use oai_rt_rs::live::*;
use serde_json::json;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_tungstenite::{
    accept_hdr_async,
    tungstenite::{
        Message,
        handshake::server::{Request, Response},
    },
};

#[test]
fn fork_start_has_required_empty_session_and_only_documented_overrides() {
    let codec = Codec::default();
    for wire in [
        json!({"type":"session.start","session":{}}),
        json!({"type":"session.start","event_id":null,"session":{"store":false}}),
        json!({"type":"session.start","session":{"audio":{"format":{"type":"audio/pcmu","rate":8000}}}}),
        json!({"type":"session.start","session":{"delegation":{"type":"responses","responses":{"model":"gpt-5.5","instructions":null,"tools":[]}}}}),
    ] {
        let event = codec.decode_fork_start(&wire.to_string()).unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&codec.encode_fork_start(&event).unwrap())
                .unwrap(),
            wire
        );
    }
    for wire in [
        json!({"type":"session.start"}),
        json!({"type":"session.start","session":null}),
        json!({"type":"session.start","session":{"model":"gpt-live-1"}}),
        json!({"type":"session.start","session":{"instructions":"new"}}),
        json!({"type":"session.start","session":{"input":[]}}),
        json!({"type":"session.start","session":{"audio":{"output":{"voice":"marin"}}}}),
        json!({"type":"session.start","session":{"client":{"data_channel":{}}}}),
        json!({"type":"session.start","session":{"delegation":{"type":"client"}}}),
        json!({"type":"session.start","session":{"delegation":null}}),
        json!({"type":"session.start","session":{"store":null}}),
    ] {
        assert!(
            codec.decode_fork_start(&wire.to_string()).is_err(),
            "{wire}"
        );
    }
    assert!(
        codec
            .decode_client(r#"{"type":"session.start","session":{}}"#)
            .is_err()
    );
}

#[test]
fn fork_transport_constraints_and_sparse_nonnullable_fields() {
    for property in ["audio", "client", "delegation", "store"] {
        let mut wire = json!({});
        wire[property] = serde_json::Value::Null;
        assert!(serde_json::from_value::<ForkSessionConfig>(wire).is_err());
    }
    let permissions = ForkSessionConfig {
        client: Some(ClientConfig {
            data_channel: DataChannelConfig::default(),
        }),
        ..ForkSessionConfig::default()
    };
    permissions.validate(ForkTransport::WebRtc).unwrap();
    assert!(permissions.validate(ForkTransport::WebSocket).is_err());
    let audio = ForkSessionConfig {
        audio: Some(ForkAudioConfig::default()),
        ..ForkSessionConfig::default()
    };
    audio.validate(ForkTransport::WebSocket).unwrap();
    assert!(audio.validate(ForkTransport::WebRtc).is_err());
    assert_eq!(audio.audio_format(), AudioFormat::Pcm { rate: 24000 });
}

#[tokio::test]
#[allow(clippy::result_large_err)]
async fn fork_websocket_sends_sparse_first_message_and_uses_new_id() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = LiveClient::with_options(
        "synthetic",
        ClientOptions {
            base_url: format!("http://{}/v1/", listener.local_addr().unwrap())
                .parse()
                .unwrap(),
            request_timeout: Duration::from_secs(3),
            ..ClientOptions::default()
        },
    )
    .unwrap();
    let peer = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut socket = accept_hdr_async(tcp, |request: &Request, response: Response| {
            assert_eq!(request.uri(), "/v1/live/sessions/stored_source/fork");
            assert_eq!(request.headers()["authorization"], "Bearer synthetic");
            Ok(response)
        })
        .await
        .unwrap();
        let Message::Text(start) = socket.next().await.unwrap().unwrap() else {
            panic!("missing start")
        };
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&start).unwrap(),
            json!({"type":"session.start","session":{"store":false}})
        );
        let session = json!({"id":"new_id","model":"gpt-live-1","expires_at":10,"status":"active","delegation":{"type":"client"}});
        socket
            .send(Message::Text(
                json!({"type":"session.started","event_id":"e","session":session})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
        let Message::Text(close) = socket.next().await.unwrap().unwrap() else {
            panic!("missing close")
        };
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&close).unwrap()["type"],
            "session.close"
        );
        socket.send(Message::Text(json!({"type":"session.closed","event_id":"e2","session":session,"reason":"close_requested","usage":{"seconds":0}}).to_string().into())).await.unwrap();
    });
    let mut fork = client
        .fork(
            "stored_source",
            ForkSessionConfig {
                store: Some(false),
                ..ForkSessionConfig::default()
            },
        )
        .await
        .unwrap();
    assert!(
        matches!(fork.next_event().await.unwrap().unwrap().event,ServerEvent::Started {session,..} if session.id=="new_id")
    );
    assert!(
        fork.send(ClientEvent::new(Command::Start {
            session: SessionConfig::default()
        }))
        .await
        .is_err()
    );
    assert!(
        fork.send(ClientEvent::new(Command::ResponseCreate))
            .await
            .is_err()
    );
    fork.close(Duration::from_secs(3), |_| Ok(()))
        .await
        .unwrap();
    peer.await.unwrap();
}
