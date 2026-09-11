use futures::{SinkExt, StreamExt};
use oai_rt_rs::live::{
    ClientEvent, ClientOptions, Command, Error, Field, LiveClient, Nullable, ServerEvent,
    SessionConfig, SessionPhase,
};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::{net::TcpListener, sync::oneshot, time::timeout};
use tokio_tungstenite::{accept_async, tungstenite::Message};

const WAIT: Duration = Duration::from_secs(5);

#[tokio::test]
async fn timed_out_write_can_still_receive_confirmed_final_usage() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = client(&listener, Duration::from_millis(300));
    let (writing_tx, writing_rx) = oneshot::channel();
    let (finish_tx, finish_rx) = oneshot::channel();
    let (observed_tx, observed_rx) = oneshot::channel();
    let peer = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut ws = accept_async(tcp).await.unwrap();
        ws.next().await.unwrap().unwrap();
        ws.send(Message::Text(started().to_string().into()))
            .await
            .unwrap();
        let mut byte = [0];
        timeout(WAIT, ws.get_ref().peek(&mut byte))
            .await
            .unwrap()
            .unwrap();
        writing_tx.send(()).unwrap();
        finish_rx.await.unwrap();
        ws.send(Message::Text(closed().to_string().into()))
            .await
            .unwrap();
        observed_rx.await.unwrap();
    });
    let mut connection = client.connect(SessionConfig::default()).await.unwrap();
    let sender = connection.sender();
    let append = tokio::spawn(async move { sender.send_audio(&vec![0; 8 * 1024 * 1024]).await });
    timeout(WAIT, writing_rx).await.unwrap().unwrap();
    assert!(matches!(
        timeout(WAIT, append).await.unwrap().unwrap(),
        Err(Error::AmbiguousWrite)
    ));
    finish_tx.send(()).unwrap();
    timeout(WAIT, async {
        loop {
            let frame = connection.next_event().await.unwrap().unwrap();
            if matches!(frame.event, ServerEvent::Closed { .. }) {
                break;
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(connection.sender().phase(), SessionPhase::Closed);
    observed_tx.send(()).unwrap();
    peer.await.unwrap();
}

#[tokio::test]
async fn observed_nullable_error_and_malformed_nonterminal_event_do_not_lose_final_usage() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = client(&listener, WAIT);
    let peer = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut ws = accept_async(tcp).await.unwrap();
        ws.next().await.unwrap().unwrap();
        ws.send(Message::Text(started().to_string().into()))
            .await
            .unwrap();
        ws.next().await.unwrap().unwrap();
        for index in 0..200 {
            ws.send(Message::Text(
                json!({
                    "type":"session.input_transcript.delta","event_id":format!("e{index}"),
                    "delta":" x ","start_ms":index,"end_ms":index+1,
                })
                .to_string()
                .into(),
            ))
            .await
            .unwrap();
        }
        for frame in [
            json!({"type":"error","event_id":"rejected","error":{
                "code":"invalid_request_error","type":"invalid_request_error","message":"synthetic",
                "param":null,"client_event_id":"backend-continue",
            }}),
            json!({"type":"session.output_transcript.delta","event_id":"bad","delta":"missing timing"}),
            closed(),
        ] {
            ws.send(Message::Text(frame.to_string().into()))
                .await
                .unwrap();
        }
    });
    let mut connection = client.connect(SessionConfig::default()).await.unwrap();
    let mut transcripts = 0;
    let mut rejections = 0;
    let mut malformed = 0;
    let final_frame = connection
        .close_with_events(WAIT, |event| {
            match event {
                Ok(frame) => match frame.event {
                    ServerEvent::InputTranscriptDelta { delta, .. } => {
                        assert_eq!(delta, " x ");
                        transcripts += 1;
                    }
                    ServerEvent::Error { error, .. } => {
                        assert_eq!(error.client_event_id.as_deref(), Some("backend-continue"));
                        assert!(error.param.is_none());
                        assert_eq!(frame.raw["error"]["param"], Value::Null);
                        rejections += 1;
                    }
                    _ => {}
                },
                Err(Error::MalformedEvent { raw, .. }) => {
                    assert_eq!(raw["type"], "session.output_transcript.delta");
                    malformed += 1;
                }
                Err(error) => return Err(error),
            }
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!((transcripts, rejections, malformed), (200, 1, 1));
    assert!(matches!(final_frame.event, ServerEvent::Closed { .. }));
    peer.await.unwrap();
}

#[tokio::test]
async fn malformed_final_event_never_confirms_usage() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = client(&listener, WAIT);
    let peer = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut ws = accept_async(tcp).await.unwrap();
        ws.next().await.unwrap().unwrap();
        ws.send(Message::Text(started().to_string().into()))
            .await
            .unwrap();
        ws.next().await.unwrap().unwrap();
        let mut final_frame = closed();
        final_frame["usage"]["seconds"] = Value::Null;
        ws.send(Message::Text(final_frame.to_string().into()))
            .await
            .unwrap();
        ws.close(None).await.unwrap();
    });
    let mut connection = client.connect(SessionConfig::default()).await.unwrap();
    let mut malformed = 0;
    let result = connection
        .close_with_events(WAIT, |event| {
            if matches!(event, Err(Error::MalformedEvent { .. })) {
                malformed += 1;
            }
            Ok(())
        })
        .await;
    assert!(matches!(result, Err(Error::UnconfirmedClose)));
    assert_eq!(malformed, 1);
    assert_eq!(connection.sender().phase(), SessionPhase::Disconnected);
    peer.await.unwrap();
}

#[tokio::test]
async fn oversized_frames_explicitly_report_continuity_loss_and_unconfirmed_usage() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = LiveClient::with_options(
        "synthetic",
        ClientOptions {
            base_url: format!("http://{}/v1/", listener.local_addr().unwrap())
                .parse()
                .unwrap(),
            codec: oai_rt_rs::live::Codec {
                max_event_bytes: 512,
            },
            ..ClientOptions::default()
        },
    )
    .unwrap();
    let peer = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut ws = accept_async(tcp).await.unwrap();
        ws.next().await.unwrap().unwrap();
        ws.send(Message::Text(started().to_string().into()))
            .await
            .unwrap();
        ws.send(Message::Text(
            json!({"type":"session.output_audio.delta","delta":"x".repeat(2048)})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
        let _ = timeout(WAIT, ws.next()).await;
    });
    let mut connection = client.connect(SessionConfig::default()).await.unwrap();
    connection.next_event().await.unwrap().unwrap();
    assert!(matches!(
        connection.next_event().await,
        Err(Error::ContinuityLost)
    ));
    assert!(matches!(
        connection.next_event().await,
        Err(Error::UnconfirmedClose)
    ));
    assert!(connection.next_event().await.unwrap().is_none());
    peer.await.unwrap();
}

fn started() -> Value {
    json!({"type":"session.started","event_id":"e","session":{
        "id":"s","model":"gpt-live-1","status":"active","expires_at":1000
    }})
}

fn closed() -> Value {
    json!({"type":"session.closed","event_id":"closed","reason":"close_requested",
        "session":{"id":"s","model":"gpt-live-1","status":"active","expires_at":1000},
        "usage":{"seconds":7.25}
    })
}

fn client(listener: &TcpListener, request_timeout: Duration) -> LiveClient {
    LiveClient::with_options(
        "synthetic",
        ClientOptions {
            base_url: format!("http://{}/v1/", listener.local_addr().unwrap())
                .parse()
                .unwrap(),
            event_capacity: 1,
            command_capacity: 1,
            request_timeout,
            ..ClientOptions::default()
        },
    )
    .unwrap()
}

#[tokio::test]
async fn blocked_sink_does_not_hide_inbound_final_usage_or_retry_audio() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = client(&listener, WAIT);
    let (writing_tx, writing_rx) = oneshot::channel();
    let (finish_tx, finish_rx) = oneshot::channel();
    let (observed_tx, observed_rx) = oneshot::channel();
    let peer = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut ws = accept_async(tcp).await.unwrap();
        ws.next().await.unwrap().unwrap();
        ws.send(Message::Text(started().to_string().into()))
            .await
            .unwrap();
        let mut byte = [0];
        timeout(WAIT, ws.get_ref().peek(&mut byte))
            .await
            .unwrap()
            .unwrap();
        writing_tx.send(()).unwrap();
        finish_rx.await.unwrap();
        ws.send(Message::Text(
            json!({"type":"session.usage.updated","event_id":"u","usage":{"seconds":7.0}})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
        ws.send(Message::Text(closed().to_string().into()))
            .await
            .unwrap();
        observed_rx.await.unwrap();
        // This listener accepted exactly one connection; no retry reconnect is used.
        assert!(
            timeout(Duration::from_millis(50), listener.accept())
                .await
                .is_err()
        );
    });
    let mut connection = client.connect(SessionConfig::default()).await.unwrap();
    let sender = connection.sender();
    let mut append =
        tokio::spawn(async move { sender.send_audio(&vec![0; 8 * 1024 * 1024]).await });
    timeout(WAIT, writing_rx).await.unwrap().unwrap();
    assert!(
        timeout(Duration::from_millis(50), &mut append)
            .await
            .is_err()
    );
    finish_tx.send(()).unwrap();
    let mut types = Vec::new();
    timeout(Duration::from_secs(1), async {
        loop {
            let frame = connection.next_event().await.unwrap().unwrap();
            types.push(frame.raw["type"].as_str().unwrap().to_owned());
            if let ServerEvent::Closed { usage, .. } = frame.event {
                assert_eq!(usage.seconds.to_bits(), 7.25_f64.to_bits());
                break;
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(
        types,
        ["session.started", "session.usage.updated", "session.closed"]
    );
    assert!(matches!(append.await.unwrap(), Err(Error::AmbiguousWrite)));
    observed_tx.send(()).unwrap();
    peer.await.unwrap();
}

#[tokio::test]
async fn close_deadline_includes_queued_command_and_blocked_writer() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = client(&listener, WAIT);
    let (writing_tx, writing_rx) = oneshot::channel();
    let (finish_tx, finish_rx) = oneshot::channel();
    let peer = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut ws = accept_async(tcp).await.unwrap();
        ws.next().await.unwrap().unwrap();
        ws.send(Message::Text(started().to_string().into()))
            .await
            .unwrap();
        let mut byte = [0];
        timeout(WAIT, ws.get_ref().peek(&mut byte))
            .await
            .unwrap()
            .unwrap();
        writing_tx.send(()).unwrap();
        finish_rx.await.unwrap();
    });
    let mut connection = client.connect(SessionConfig::default()).await.unwrap();
    let sender = connection.sender();
    let append = tokio::spawn(async move { sender.send_audio(&vec![0; 8 * 1024 * 1024]).await });
    timeout(WAIT, writing_rx).await.unwrap().unwrap();
    let result = timeout(
        Duration::from_secs(1),
        connection.close(Duration::from_millis(50), |_| Ok(())),
    )
    .await
    .unwrap();
    assert!(matches!(result, Err(Error::Timeout)));
    assert_eq!(connection.sender().phase(), SessionPhase::Disconnected);
    assert!(matches!(
        timeout(WAIT, append).await.unwrap().unwrap(),
        Err(Error::AmbiguousWrite)
    ));
    finish_tx.send(()).unwrap();
    peer.await.unwrap();
}

#[tokio::test]
async fn earlier_rejection_does_not_stop_close_or_lose_backend_completion() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = client(&listener, WAIT);
    let peer = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut ws = accept_async(tcp).await.unwrap();
        ws.next().await.unwrap().unwrap();
        ws.send(Message::Text(started().to_string().into()))
            .await
            .unwrap();
        ws.next().await.unwrap().unwrap();
        ws.next().await.unwrap().unwrap();
        for frame in [
            json!({"type":"error","event_id":"rejection","error":{"type":"invalid_request_error","code":"unknown_delegation","message":"synthetic","client_event_id":"earlier"}}),
            json!({"type":"response.event","event_id":"backend","event":{"type":"response.completed","response":{"id":"r","output":[]}}}),
            json!({"type":"session.usage.updated","event_id":"u","usage":{"seconds":7.0}}),
            closed(),
        ] {
            ws.send(Message::Text(frame.to_string().into()))
                .await
                .unwrap();
        }
    });
    let mut connection = client.connect(SessionConfig::default()).await.unwrap();
    connection
        .send(ClientEvent {
            event_id: Field::Value("earlier".into()),
            command: Command::ThinkingAppend {
                content: "synthetic".into(),
                delegation_id: Nullable(Some("unknown".into())),
            },
        })
        .await
        .unwrap();
    let mut types = Vec::new();
    let frame = connection
        .close(WAIT, |frame| {
            types.push(frame.raw["type"].as_str().unwrap().to_owned());
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(
        types,
        [
            "session.started",
            "error",
            "response.event",
            "session.usage.updated",
            "session.closed"
        ]
    );
    assert!(matches!(frame.event, ServerEvent::Closed { .. }));
    peer.await.unwrap();
}

#[tokio::test]
async fn racing_remote_close_with_full_event_queue_resolves_all_senders() {
    for _ in 0..20 {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = client(&listener, WAIT);
        let peer = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut ws = accept_async(tcp).await.unwrap();
            ws.next().await.unwrap().unwrap();
            ws.send(Message::Text(started().to_string().into()))
                .await
                .unwrap();
            ws.send(Message::Text(
                json!({"type":"info","event_id":"i","code":"notice","message":"synthetic"})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
            ws.send(Message::Text(closed().to_string().into()))
                .await
                .unwrap();
            let _ = timeout(WAIT, ws.next()).await;
        });
        let mut connection = client.connect(SessionConfig::default()).await.unwrap();
        let sender = connection.sender();
        let mut queued = Box::pin(sender.send(ClientEvent::new(Command::InputAudioMute)));
        let initial = futures::poll!(&mut queued);
        let mut count = 0;
        let terminal = timeout(
            Duration::from_secs(1),
            connection.close(Duration::from_millis(500), |_| {
                count += 1;
                Ok(())
            }),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(matches!(terminal.event, ServerEvent::Closed { .. }));
        assert_eq!(count, 3);
        if initial.is_pending() {
            let _ = timeout(Duration::from_secs(1), queued).await.unwrap();
        }
        peer.await.unwrap();
    }
}
