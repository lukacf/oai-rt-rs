use futures::{SinkExt, StreamExt};
use oai_rt_rs::live::*;
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    net::{TcpListener, TcpStream},
    task::JoinHandle,
    time::timeout,
};
use tokio_tungstenite::{
    WebSocketStream, accept_hdr_async,
    tungstenite::{
        Message,
        handshake::server::{Request, Response},
    },
};

const WAIT: Duration = Duration::from_secs(3);
type Peer = WebSocketStream<TcpStream>;

#[allow(clippy::result_large_err)]
async fn server<F, Fut>(handler: F) -> (LiveClient, JoinHandle<()>)
where
    F: FnOnce(Peer) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let options = ClientOptions {
        base_url: format!("http://{}/v1/", listener.local_addr().unwrap())
            .parse()
            .unwrap(),
        request_timeout: WAIT,
        event_capacity: 1,
        command_capacity: 1,
        organization: Some("org-test".into()),
        project: Some("proj-test".into()),
        ..ClientOptions::default()
    };
    let task = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let ws = accept_hdr_async(stream, |request: &Request, response: Response| {
            assert_eq!(
                request.headers()["authorization"],
                "Bearer synthetic-test-secret"
            );
            assert_eq!(request.headers()["openai-organization"], "org-test");
            assert_eq!(request.headers()["openai-project"], "proj-test");
            assert!(!request.headers().contains_key("openai-beta"));
            assert!(!request.headers().contains_key("openai-alpha"));
            assert!(request.uri().query().is_none());
            assert!(
                request.uri().path() == "/v1/live/sessions"
                    || request.uri().path() == "/v1/live/sessions/live_test/attach"
            );
            Ok(response)
        })
        .await
        .unwrap();
        handler(ws).await;
    });
    (
        LiveClient::with_options("synthetic-test-secret", options).unwrap(),
        task,
    )
}

async fn recv(ws: &mut Peer) -> Value {
    loop {
        match timeout(WAIT, ws.next()).await.unwrap().unwrap().unwrap() {
            Message::Text(text) => return serde_json::from_str(&text).unwrap(),
            Message::Ping(_) => ws.flush().await.unwrap(),
            other => panic!("unexpected frame {other:?}"),
        }
    }
}

async fn send(ws: &mut Peer, value: Value) {
    ws.send(Message::Text(value.to_string().into()))
        .await
        .unwrap();
}

fn snapshot() -> Value {
    json!({"id":"live_test","model":"gpt-live-1","status":"active","expires_at":12345.5})
}

async fn started(ws: &mut Peer) {
    assert_eq!(
        recv(ws).await,
        json!({"type":"session.start","session":{"model":"gpt-live-1"}})
    );
    send(
        ws,
        json!({"type":"session.started","event_id":"start","session":snapshot()}),
    )
    .await;
}

async fn closed(ws: &mut Peer) {
    send(ws, json!({"type":"session.closed","event_id":"close","session":snapshot(),"reason":"close_requested","usage":{"seconds":2.5}})).await;
}

#[tokio::test]
async fn many_delegations_with_capacity_one_do_not_require_retained_id_history() {
    let (client, peer) = server(|mut ws| async move {
        started(&mut ws).await;
        for i in 0..512 {
            send(&mut ws, json!({"type":"session.delegation.created","event_id":format!("e{i}"),"offset_ms":0.0,
                "delegation":{"type":"delegation","id":format!("d{i}{}", "x".repeat(1024)),"target":"client"}})).await;
            assert_eq!(recv(&mut ws).await["type"], "session.input_audio.mute");
        }
        assert_eq!(recv(&mut ws).await["delegation_id"], "not-replayed");
        assert_eq!(recv(&mut ws).await["type"], "session.close");
        closed(&mut ws).await;
    }).await;
    let mut connection = client.connect(SessionConfig::default()).await.unwrap();
    connection.next_event().await.unwrap().unwrap();
    for _ in 0..512 {
        assert!(matches!(
            connection.next_event().await.unwrap().unwrap().event,
            ServerEvent::DelegationCreated { .. }
        ));
        connection
            .send(ClientEvent::new(Command::InputAudioMute))
            .await
            .unwrap();
    }
    connection
        .send(ClientEvent::new(Command::ThinkingAppend {
            content: "synthetic".into(),
            delegation_id: Nullable(Some("not-replayed".into())),
        }))
        .await
        .unwrap();
    connection.close(WAIT, |_| Ok(())).await.unwrap();
    peer.await.unwrap();
}

#[tokio::test]
async fn startup_gates_commands_and_graceful_close_preserves_every_event() {
    let (client, peer) = server(|mut ws| async move {
        started(&mut ws).await;
        assert_eq!(recv(&mut ws).await["type"], "session.input_audio.append");
        assert_eq!(recv(&mut ws).await["type"], "session.close");
        send(
            &mut ws,
            json!({"type":"session.usage.updated","event_id":"usage","usage":{"seconds":2.0}}),
        )
        .await;
        closed(&mut ws).await;
    })
    .await;
    let mut connection = client.connect(SessionConfig::default()).await.unwrap();
    assert_eq!(connection.sender().phase(), SessionPhase::Active);
    connection.sender().send_audio(&[0, 0]).await.unwrap();
    assert!(
        connection
            .send(ClientEvent::new(Command::Start {
                session: SessionConfig::default()
            }))
            .await
            .is_err()
    );
    let mut observed = Vec::new();
    let final_frame = connection
        .close(WAIT, |frame| {
            observed.push(frame.raw["type"].as_str().unwrap().to_owned());
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(
        observed,
        ["session.started", "session.usage.updated", "session.closed"]
    );
    assert!(matches!(
        final_frame.event,
        ServerEvent::Closed {
            usage: Usage { seconds: 2.5 },
            ..
        }
    ));
    assert!(matches!(
        connection
            .send(ClientEvent::new(Command::InputAudioMute))
            .await,
        Err(Error::Closed)
    ));
    assert!(connection.next_event().await.unwrap().is_none());
    peer.await.unwrap();
}

#[tokio::test]
async fn split_backpressure_does_not_make_close_wait_for_an_audio_ack() {
    let (client, peer) = server(|mut ws| async move {
        started(&mut ws).await;
        send(
            &mut ws,
            json!({"type":"session.output_audio.delta","delta":"AAA="}),
        )
        .await;
        assert_eq!(recv(&mut ws).await["type"], "session.input_audio.append");
        assert_eq!(recv(&mut ws).await["type"], "session.close");
        closed(&mut ws).await;
    })
    .await;
    let connection = client.connect(SessionConfig::default()).await.unwrap();
    let (sender, mut receiver) = connection.split();
    timeout(WAIT, sender.send_audio(&[0, 0]))
        .await
        .unwrap()
        .unwrap();
    timeout(WAIT, sender.send(ClientEvent::new(Command::Close)))
        .await
        .unwrap()
        .unwrap();
    let mut kinds = Vec::new();
    while let Some(frame) = timeout(WAIT, receiver.next_event()).await.unwrap().unwrap() {
        kinds.push(frame.raw["type"].as_str().unwrap().to_owned());
    }
    assert_eq!(
        kinds,
        [
            "session.started",
            "session.output_audio.delta",
            "session.closed"
        ]
    );
    peer.await.unwrap();
}

#[tokio::test]
async fn startup_rejections_are_provider_errors_not_false_success() {
    let (client, peer) = server(|mut ws| async move {
        assert_eq!(recv(&mut ws).await["type"], "session.start");
        send(&mut ws, json!({"type":"error","event_id":"error","error":{"type":"invalid_request_error","code":"bad_model","message":"synthetic"}})).await;
    }).await;
    let result = client.connect(SessionConfig::default()).await;
    assert!(
        matches!(result, Err(Error::Provider(error)) if error.code.as_deref() == Some("bad_model"))
    );
    peer.await.unwrap();
}

#[tokio::test]
async fn unexpected_disconnect_is_unconfirmed_and_never_retried() {
    let starts = Arc::new(Mutex::new(0));
    let recorded = starts.clone();
    let (client, peer) = server(move |mut ws| async move {
        started(&mut ws).await;
        *recorded.lock().unwrap() += 1;
        ws.close(None).await.unwrap();
    })
    .await;
    let mut connection = client.connect(SessionConfig::default()).await.unwrap();
    assert!(matches!(
        connection.next_event().await.unwrap().unwrap().event,
        ServerEvent::Started { .. }
    ));
    assert!(matches!(
        connection.next_event().await,
        Err(Error::UnconfirmedClose)
    ));
    assert_eq!(*starts.lock().unwrap(), 1);
    peer.await.unwrap();
}

#[tokio::test]
async fn sideband_has_no_startup_command_and_reflected_audio_is_not_send_permission() {
    let (client, peer) = server(|mut ws| async move {
        send(&mut ws, json!({"type":"session.input_audio.append","audio":"AAA="})).await;
        let command = recv(&mut ws).await;
        assert_eq!(command["type"], "session.thinking.append");
        assert_eq!(command["delegation_id"], Value::Null);
        send(&mut ws, json!({"type":"session.thinking.appended","event_id":"ack","client_event_id":"context","start_ms":0.5,"end_ms":0.5})).await;
        assert_eq!(recv(&mut ws).await["type"], "session.close");
        closed(&mut ws).await;
    }).await;
    let mut connection = client.attach("live_test").await.unwrap();
    let sender = connection.sender();
    assert_eq!(sender.role(), ConnectionRole::Sideband);
    assert!(sender.send_audio(&[0, 0]).await.is_err());
    assert!(
        sender
            .send(ClientEvent::new(Command::InputAudioAppend {
                audio: "AAA=".into()
            }))
            .await
            .is_err()
    );
    assert!(
        sender
            .send(ClientEvent::new(Command::Start {
                session: SessionConfig::default()
            }))
            .await
            .is_err()
    );
    sender
        .send(ClientEvent {
            event_id: Field::Value("context".into()),
            command: Command::ThinkingAppend {
                content: "synthetic".into(),
                delegation_id: Nullable(None),
            },
        })
        .await
        .unwrap();
    assert!(matches!(
        connection.next_event().await.unwrap().unwrap().event,
        ServerEvent::InputAudio { .. }
    ));
    assert_eq!(
        connection
            .next_event()
            .await
            .unwrap()
            .unwrap()
            .client_event_id
            .as_deref(),
        Some("context")
    );
    connection.close(WAIT, |_| Ok(())).await.unwrap();
    peer.await.unwrap();
}

#[tokio::test]
async fn dropping_receiver_releases_socket_even_with_cloned_senders() {
    let (client, peer) = server(|mut ws| async move {
        started(&mut ws).await;
        let message = timeout(WAIT, ws.next()).await.unwrap();
        assert!(!matches!(message, Some(Ok(Message::Text(_)))));
    })
    .await;
    let (sender, receiver) = client
        .connect(SessionConfig::default())
        .await
        .unwrap()
        .split();
    drop(receiver);
    assert!(matches!(
        sender.send(ClientEvent::new(Command::Close)).await,
        Err(Error::Closed)
    ));
    peer.await.unwrap();
}

#[test]
fn auth_and_endpoint_failures_do_not_print_credentials() {
    let client = LiveClient::new("synthetic-private-secret").unwrap();
    assert!(!format!("{client:?}").contains("synthetic-private-secret"));
    for key in ["", "secret\r\nAuthorization: stolen"] {
        let error = LiveClient::new(key).err().unwrap();
        assert!(!format!("{error:?}").contains("stolen"));
    }
    for root in [
        "http://example.com/v1/",
        "https://name:secret@example.com/v1/",
        "https://example.com/v1/?key=secret",
    ] {
        let options = ClientOptions {
            base_url: root.parse().unwrap(),
            ..ClientOptions::default()
        };
        assert!(LiveClient::with_options("key", options).is_err());
    }
}

#[tokio::test]
async fn malformed_nonterminal_event_is_visible_before_valid_final_usage() {
    let (client, peer) = server(|mut ws| async move {
            started(&mut ws).await;
            send(&mut ws, json!({"type":"session.output_transcript.delta","event_id":"broken","delta":"missing timestamps"})).await;
            closed(&mut ws).await;
        }).await;
    let mut connection = client.connect(SessionConfig::default()).await.unwrap();
    connection.next_event().await.unwrap().unwrap();
    assert!(matches!(
        connection.next_event().await,
        Err(Error::MalformedEvent { .. })
    ));
    assert!(matches!(
        connection.next_event().await.unwrap().unwrap().event,
        ServerEvent::Closed { .. }
    ));
    assert!(connection.next_event().await.unwrap().is_none());
    assert_eq!(connection.sender().phase(), SessionPhase::Closed);
    peer.await.unwrap();
}

#[tokio::test]
async fn closing_deadline_releases_peer_without_claiming_final_usage() {
    let (client, peer) = server(|mut ws| async move {
        started(&mut ws).await;
        assert_eq!(recv(&mut ws).await["type"], "session.close");
        let result = timeout(WAIT, ws.next()).await.unwrap();
        assert!(!matches!(result, Some(Ok(Message::Text(_)))));
    })
    .await;
    let mut connection = client.connect(SessionConfig::default()).await.unwrap();
    assert!(matches!(
        connection
            .close(Duration::from_millis(50), |_| Ok(()))
            .await,
        Err(Error::Timeout)
    ));
    assert_eq!(connection.sender().phase(), SessionPhase::Disconnected);
    peer.await.unwrap();
}

#[tokio::test]
async fn immutable_modes_reject_locally_without_sending_a_command() {
    let (client, peer) = server(|mut ws| async move {
        let start = recv(&mut ws).await;
        assert_eq!(start["session"]["delegation"]["type"], "responses");
        send(
            &mut ws,
            json!({"type":"session.started","event_id":"e","session":snapshot()}),
        )
        .await;
        assert_eq!(recv(&mut ws).await["type"], "session.close");
        closed(&mut ws).await;
    })
    .await;
    let mut connection = client
        .connect(SessionConfig {
            delegation: Field::Value(DelegationConfig::Responses {
                responses: ResponsesConfig {
                    model: "gpt-5.5".into(),
                    options: ResponsesOptions::default(),
                },
            }),
            ..SessionConfig::default()
        })
        .await
        .unwrap();
    assert!(matches!(
        connection
            .send(ClientEvent::new(Command::Update {
                session: SessionUpdate {
                    delegation: Field::Null
                },
            }))
            .await,
        Err(Error::Invalid(_))
    ));
    connection.close(WAIT, |_| Ok(())).await.unwrap();
    peer.await.unwrap();
}

#[tokio::test]
async fn usage_snapshots_remain_cumulative_and_append_rejections_correlate() {
    let (client, peer) = server(|mut ws| async move {
            started(&mut ws).await;
            for seconds in [1.5, 1.5, 2.25] {
                send(&mut ws, json!({"type":"session.usage.updated","event_id":"usage","usage":{"seconds":seconds}})).await;
            }
            let command = recv(&mut ws).await;
            assert_eq!(command["event_id"], "invalid-id");
            send(&mut ws, json!({"type":"error","event_id":"reject","error":{
                "type":"invalid_request_error","code":"unknown_delegation","message":"synthetic","client_event_id":"invalid-id",
            }})).await;
            assert_eq!(recv(&mut ws).await["type"], "session.close");
            closed(&mut ws).await;
        }).await;
    let mut connection = client.connect(SessionConfig::default()).await.unwrap();
    let sender = connection.sender();
    sender
        .send(ClientEvent {
            event_id: Field::Value("invalid-id".into()),
            command: Command::ThinkingAppend {
                content: "synthetic".into(),
                delegation_id: Nullable(Some("unknown".into())),
            },
        })
        .await
        .unwrap();
    let mut snapshots = Vec::new();
    loop {
        match connection.next_event().await.unwrap().unwrap().event {
            ServerEvent::UsageUpdated { usage, .. } => snapshots.push(usage.seconds),
            ServerEvent::Error { error, .. } => {
                assert_eq!(error.client_event_id.as_deref(), Some("invalid-id"));
                break;
            }
            _ => {}
        }
    }
    assert_eq!(snapshots, [1.5, 1.5, 2.25]);
    connection.close(WAIT, |_| Ok(())).await.unwrap();
    peer.await.unwrap();
}

#[tokio::test]
async fn an_already_cancelled_queued_send_is_not_written() {
    let (client, peer) = server(|mut ws| async move {
        started(&mut ws).await;
        assert_eq!(recv(&mut ws).await["type"], "session.close");
        closed(&mut ws).await;
    })
    .await;
    let mut connection = client.connect(SessionConfig::default()).await.unwrap();
    let sender = connection.sender();
    let mut cancelled = Box::pin(sender.send(ClientEvent::new(Command::InputAudioMute)));
    assert!(futures::poll!(&mut cancelled).is_pending());
    drop(cancelled);
    connection.close(WAIT, |_| Ok(())).await.unwrap();
    peer.await.unwrap();
}
