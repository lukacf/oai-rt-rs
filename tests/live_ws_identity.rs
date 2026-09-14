use futures::{SinkExt, StreamExt};
use oai_rt_rs::live::*;
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::oneshot,
    task::JoinHandle,
    time::timeout,
};
use tokio_tungstenite::{WebSocketStream, accept_async, tungstenite::Message};

const WAIT: Duration = Duration::from_secs(5);
type Peer = WebSocketStream<TcpStream>;

async fn server<F, Fut>(handler: F) -> (LiveClient, JoinHandle<()>)
where
    F: FnOnce(Peer) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = LiveClient::with_options(
        "synthetic",
        ClientOptions {
            base_url: format!("http://{}/v1/", listener.local_addr().unwrap())
                .parse()
                .unwrap(),
            request_timeout: WAIT,
            event_capacity: 1,
            command_capacity: 1,
            ..ClientOptions::default()
        },
    )
    .unwrap();
    let peer = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        handler(accept_async(stream).await.unwrap()).await;
    });
    (client, peer)
}

fn snapshot(kind: &str, id: &str) -> Value {
    let mut frame = json!({
        "type":kind,"event_id":"snapshot",
        "session":{"id":id,"model":"gpt-live-1","status":"active","expires_at":1234},
    });
    if kind == "session.closed" {
        frame["reason"] = json!("close_requested");
        frame["usage"] = json!({"seconds":3.25});
    }
    frame
}

async fn send(peer: &mut Peer, value: Value) {
    peer.send(Message::Text(value.to_string().into()))
        .await
        .unwrap();
}

async fn recv(peer: &mut Peer) -> Value {
    let message = timeout(WAIT, peer.next()).await.unwrap().unwrap().unwrap();
    let Message::Text(text) = message else {
        panic!("expected command");
    };
    serde_json::from_str(&text).unwrap()
}

async fn no_more_commands(peer: &mut Peer) {
    // A local abort must not even flush a WebSocket close to a mismatched peer.
    let message = timeout(WAIT, peer.next()).await.unwrap();
    assert!(matches!(message, None | Some(Err(_))));
}

fn mismatch(error: Error, expected: Option<&str>, kind: &str) -> Arc<SessionIdentityMismatch> {
    let Error::SessionIdentityMismatch(failure) = error else {
        panic!("expected identity failure, got {error}");
    };
    assert_eq!(failure.expected_session_id.as_deref(), expected);
    assert_eq!(failure.observed_session_id, "alien-private-id");
    assert_eq!(failure.raw, snapshot(kind, "alien-private-id"));
    assert!(!format!("{failure:?}").contains("alien-private-id"));
    assert!(
        !format!("{:?}", Error::SessionIdentityMismatch(failure.clone()))
            .contains("alien-private-id")
    );
    failure
}

#[tokio::test]
async fn primary_and_sideband_reject_alien_snapshots_without_accepting_final_usage() {
    for role in [ConnectionRole::Primary, ConnectionRole::Sideband] {
        for kind in ["session.started", "session.updated", "session.closed"] {
            let (inject, injected) = oneshot::channel();
            let (client, peer) = server(move |mut peer| async move {
                if role == ConnectionRole::Primary {
                    assert_eq!(recv(&mut peer).await["type"], "session.start");
                }
                send(&mut peer, snapshot("session.started", "bound")).await;
                injected.await.unwrap();
                send(&mut peer, snapshot(kind, "alien-private-id")).await;
                no_more_commands(&mut peer).await;
            })
            .await;
            let mut connection = match role {
                ConnectionRole::Primary => client.connect(SessionConfig::default()).await.unwrap(),
                ConnectionRole::Sideband => client.attach("bound").await.unwrap(),
            };
            assert!(matches!(
                connection.next_event().await.unwrap().unwrap().event,
                ServerEvent::Started { .. }
            ));
            inject.send(()).unwrap();
            let failure = mismatch(
                timeout(WAIT, connection.next_event())
                    .await
                    .unwrap()
                    .unwrap_err(),
                Some("bound"),
                kind,
            );
            assert_eq!(connection.sender().phase(), SessionPhase::Disconnected);
            for command in [Command::InputAudioMute, Command::Close] {
                let error = connection
                    .send(ClientEvent::new(command))
                    .await
                    .unwrap_err();
                assert!(Arc::ptr_eq(&failure, &mismatch(error, Some("bound"), kind)));
            }
            let mut observed = 0;
            let error = connection
                .close_with_events(WAIT, |event| {
                    mismatch(event.unwrap_err(), Some("bound"), kind);
                    observed += 1;
                    Ok(())
                })
                .await
                .unwrap_err();
            assert_eq!(
                observed, 1,
                "swallowing the error must not spin until timeout"
            );
            mismatch(error, Some("bound"), kind);
            for _ in 0..2 {
                mismatch(
                    connection.disconnect().await.unwrap_err(),
                    Some("bound"),
                    kind,
                );
                mismatch(
                    connection.next_event().await.unwrap_err(),
                    Some("bound"),
                    kind,
                );
            }
            peer.await.unwrap();
        }
    }
}

#[tokio::test]
async fn sideband_is_bound_before_any_started_event_or_observer_readiness() {
    for kind in ["session.started", "session.closed"] {
        let (client, peer) = server(move |mut peer| async move {
            send(&mut peer, snapshot(kind, "alien-private-id")).await;
            no_more_commands(&mut peer).await;
        })
        .await;
        let mut connection = client.attach("bound").await.unwrap();
        mismatch(
            timeout(WAIT, connection.next_event())
                .await
                .unwrap()
                .unwrap_err(),
            Some("bound"),
            kind,
        );
        assert_eq!(connection.sender().phase(), SessionPhase::Disconnected);
        mismatch(
            connection
                .send(ClientEvent::new(Command::Close))
                .await
                .unwrap_err(),
            Some("bound"),
            kind,
        );
        peer.await.unwrap();
    }
}

#[tokio::test]
async fn primary_cannot_bind_from_closed_or_updated_before_started() {
    for kind in ["session.closed", "session.updated"] {
        let (client, peer) = server(move |mut peer| async move {
            assert_eq!(recv(&mut peer).await["type"], "session.start");
            send(&mut peer, snapshot(kind, "alien-private-id")).await;
            no_more_commands(&mut peer).await;
        })
        .await;
        let error = client
            .connect(SessionConfig::default())
            .await
            .err()
            .unwrap();
        mismatch(error, None, kind);
        peer.await.unwrap();
    }
}

#[tokio::test]
async fn primary_empty_identity_and_fork_source_identity_never_become_ready() {
    for (id, fork) in [("", false), ("stored-source", true)] {
        let (client, peer) = server(move |mut peer| async move {
            assert_eq!(recv(&mut peer).await["type"], "session.start");
            send(&mut peer, snapshot("session.started", id)).await;
            no_more_commands(&mut peer).await;
        })
        .await;
        let result = if fork {
            client.fork(id, ForkSessionConfig::default()).await
        } else {
            client.connect(SessionConfig::default()).await
        };
        let Some(Error::SessionIdentityMismatch(failure)) = result.err() else {
            panic!("unexpected startup result");
        };
        assert_eq!(failure.expected_session_id, None);
        assert_eq!(failure.observed_session_id, id);
        peer.await.unwrap();
    }
}

#[tokio::test]
async fn same_id_replay_and_identityless_traffic_preserve_bytes_and_final_usage() {
    for role in [ConnectionRole::Primary, ConnectionRole::Sideband] {
        let (client, peer) = server(move |mut peer| async move {
            if role == ConnectionRole::Primary {
                assert_eq!(recv(&mut peer).await["type"], "session.start");
                send(&mut peer, snapshot("session.started", "bound")).await;
            }
            // An attached sideband need not wait for a started replay.
            assert_eq!(recv(&mut peer).await["content"], "  exact\r\ntext\t ");
            for kind in ["session.started", "session.started", "session.updated"] {
                send(&mut peer, snapshot(kind, "bound")).await;
            }
            send(
                &mut peer,
                json!({"type":"session.output_transcript.delta","event_id":"text",
                    "delta":"  exact\r\ntext\t ","start_ms":0,"end_ms":1}),
            )
            .await;
            send(
                &mut peer,
                json!({"type":"session.usage.updated","event_id":"usage","usage":{"seconds":2}}),
            )
            .await;
            assert_eq!(recv(&mut peer).await["type"], "session.close");
            send(&mut peer, snapshot("session.closed", "bound")).await;
        })
        .await;
        let mut connection = match role {
            ConnectionRole::Primary => client.connect(SessionConfig::default()).await.unwrap(),
            ConnectionRole::Sideband => client.attach("bound").await.unwrap(),
        };
        connection
            .send(ClientEvent::new(Command::ThinkingAppend {
                content: "  exact\r\ntext\t ".into(),
                delegation_id: Nullable(None),
            }))
            .await
            .unwrap();
        let mut started = 0;
        let final_frame = connection
            .close(WAIT, |frame| {
                match &frame.event {
                    ServerEvent::Started { .. } => started += 1,
                    ServerEvent::OutputTranscriptDelta { delta, .. } => {
                        assert_eq!(delta, "  exact\r\ntext\t ");
                    }
                    _ => {}
                }
                Ok(())
            })
            .await
            .unwrap();
        assert_eq!(
            started,
            if role == ConnectionRole::Primary {
                3
            } else {
                2
            }
        );
        assert_eq!(final_frame.raw, snapshot("session.closed", "bound"));
        assert_eq!(connection.sender().phase(), SessionPhase::Closed);
        assert!(connection.next_event().await.unwrap().is_none());
        connection.disconnect().await.unwrap();
        assert!(connection.next_event().await.unwrap().is_none());
        peer.await.unwrap();
    }
}

#[tokio::test]
async fn conflicting_malformed_snapshot_cannot_evade_the_identity_fence() {
    let (client, peer) = server(|mut peer| async move {
        let mut frame = snapshot("session.closed", "alien-private-id");
        frame["usage"] = Value::Null;
        send(&mut peer, frame).await;
        no_more_commands(&mut peer).await;
    })
    .await;
    let mut connection = client.attach("bound").await.unwrap();
    let Error::SessionIdentityMismatch(failure) = connection.next_event().await.unwrap_err() else {
        panic!("identity evidence was hidden by malformed usage");
    };
    assert_eq!(failure.observed_session_id, "alien-private-id");
    assert_eq!(failure.raw["usage"], Value::Null);
    assert_eq!(connection.sender().phase(), SessionPhase::Disconnected);
    peer.await.unwrap();
}

#[tokio::test]
async fn accepted_conflict_fences_queued_and_waiting_commands_but_inflight_stays_ambiguous() {
    for role in [ConnectionRole::Primary, ConnectionRole::Sideband] {
        for kind in ["session.started", "session.closed"] {
            let (writing, written) = oneshot::channel();
            let (inject, injected) = oneshot::channel();
            let (fenced, fence) = oneshot::channel();
            let (client, peer) = server(move |mut peer| async move {
                if role == ConnectionRole::Primary {
                    assert_eq!(recv(&mut peer).await["type"], "session.start");
                }
                send(&mut peer, snapshot("session.started", "bound")).await;
                let mut byte = [0];
                timeout(WAIT, peer.get_ref().peek(&mut byte))
                    .await
                    .unwrap()
                    .unwrap();
                writing.send(()).unwrap();
                injected.await.unwrap();
                send(&mut peer, snapshot(kind, "alien-private-id")).await;
                fence.await.unwrap();
                // The first frame may have begun before conflict observation.
                // A partial frame may fail, but no following command may exist.
                match timeout(WAIT, peer.next()).await.unwrap() {
                    Some(Ok(Message::Text(text))) => {
                        let frame: Value = serde_json::from_str(&text).unwrap();
                        assert_eq!(frame["type"], "session.thinking.append");
                        no_more_commands(&mut peer).await;
                    }
                    None | Some(Err(_)) => {}
                    other => panic!("unexpected post-conflict frame: {other:?}"),
                }
            })
            .await;
            let mut connection = match role {
                ConnectionRole::Primary => client.connect(SessionConfig::default()).await.unwrap(),
                ConnectionRole::Sideband => client.attach("bound").await.unwrap(),
            };
            connection.next_event().await.unwrap().unwrap();
            let sender = connection.sender();
            let writing_sender = sender.clone();
            let inflight = tokio::spawn(async move {
                writing_sender
                    .send(ClientEvent::new(Command::ThinkingAppend {
                        content: "x".repeat(12 * 1024 * 1024),
                        delegation_id: Nullable(None),
                    }))
                    .await
            });
            timeout(WAIT, written).await.unwrap().unwrap();
            let mut queued = Box::pin(sender.send(ClientEvent::new(Command::Close)));
            assert!(futures::poll!(&mut queued).is_pending());
            let mut unadmitted = Box::pin(sender.send(ClientEvent::new(Command::InputAudioMute)));
            assert!(futures::poll!(&mut unadmitted).is_pending());
            inject.send(()).unwrap();
            // Receipts synchronize with driver observation, not observer delivery.
            mismatch(
                timeout(WAIT, queued).await.unwrap().unwrap_err(),
                Some("bound"),
                kind,
            );
            mismatch(
                timeout(WAIT, unadmitted).await.unwrap().unwrap_err(),
                Some("bound"),
                kind,
            );
            assert!(matches!(
                timeout(WAIT, inflight).await.unwrap().unwrap(),
                Err(Error::AmbiguousWrite)
            ));
            assert_eq!(sender.phase(), SessionPhase::Disconnected);
            fenced.send(()).unwrap();
            mismatch(
                connection.next_event().await.unwrap_err(),
                Some("bound"),
                kind,
            );
            peer.await.unwrap();
        }
    }
}

#[tokio::test]
async fn mismatch_disconnect_releases_socket_with_full_event_queue_and_retains_evidence() {
    let (client, peer) = server(|mut peer| async move {
        send(
            &mut peer,
            json!({"type":"session.usage.updated","event_id":"u","usage":{"seconds":1}}),
        )
        .await;
        send(&mut peer, snapshot("session.closed", "alien-private-id")).await;
        no_more_commands(&mut peer).await;
    })
    .await;
    let mut connection = client.attach("bound").await.unwrap();
    // Peer EOF proves the driver accepted the mismatch while the event queue
    // still holds the earlier usage event and no observer has drained anything.
    timeout(WAIT, peer).await.unwrap().unwrap();
    mismatch(
        timeout(WAIT, connection.disconnect())
            .await
            .unwrap()
            .unwrap_err(),
        Some("bound"),
        "session.closed",
    );
    assert!(matches!(
        connection.next_event().await.unwrap().unwrap().event,
        ServerEvent::UsageUpdated { .. }
    ));
    mismatch(
        connection.next_event().await.unwrap_err(),
        Some("bound"),
        "session.closed",
    );
}

#[tokio::test]
async fn cancelling_close_aborts_local_transport_without_fabricating_remote_close() {
    let (close_seen, seen) = oneshot::channel();
    let (client, peer) = server(move |mut peer| async move {
        assert_eq!(recv(&mut peer).await["type"], "session.close");
        close_seen.send(()).unwrap();
        no_more_commands(&mut peer).await;
    })
    .await;
    let mut connection = client.attach("bound").await.unwrap();
    let mut close = Box::pin(connection.close(WAIT, |_| Ok(())));
    tokio::select! {
        result = &mut close => panic!("close unexpectedly finished: {result:?}"),
        result = seen => result.unwrap(),
    }
    drop(close);
    assert_eq!(connection.sender().phase(), SessionPhase::Disconnected);
    assert!(matches!(
        timeout(WAIT, connection.next_event()).await.unwrap(),
        Err(Error::UnconfirmedClose)
    ));
    assert!(connection.next_event().await.unwrap().is_none());
    peer.await.unwrap();
}

#[tokio::test]
async fn cancelling_pending_local_disconnect_still_aborts_full_queue_driver() {
    let (ready, ready_rx) = oneshot::channel();
    let (client, peer) = server(move |mut peer| async move {
        for i in 0..4 {
            send(
                &mut peer,
                json!({"type":"session.usage.updated","event_id":format!("u{i}"),"usage":{"seconds":i}}),
            )
            .await;
        }
        ready.send(()).unwrap();
        no_more_commands(&mut peer).await;
    })
    .await;
    let connection = client.attach("bound").await.unwrap();
    ready_rx.await.unwrap();
    let (sender, mut receiver) = connection.split();
    let mut disconnect = Box::pin(receiver.disconnect());
    assert!(futures::poll!(&mut disconnect).is_pending());
    drop(disconnect);
    assert_eq!(sender.phase(), SessionPhase::Disconnected);
    assert!(matches!(
        sender.send(ClientEvent::new(Command::Close)).await,
        Err(Error::Closed)
    ));
    timeout(WAIT, async {
        loop {
            match receiver.next_event().await {
                Ok(Some(frame)) => assert!(matches!(frame.event, ServerEvent::UsageUpdated { .. })),
                Err(Error::UnconfirmedClose) => break,
                result => panic!("abort lost its unconfirmed stream fate: {result:?}"),
            }
        }
    })
    .await
    .unwrap();
    assert!(receiver.next_event().await.unwrap().is_none());
    assert!(receiver.next_event().await.unwrap().is_none());
    peer.await.unwrap();
}

#[tokio::test]
async fn disconnect_after_reported_remote_eof_does_not_repeat_terminal_error() {
    let (client, peer) = server(|mut peer| async move {
        peer.close(None).await.unwrap();
    })
    .await;
    let mut connection = client.attach("bound").await.unwrap();
    assert!(matches!(
        timeout(WAIT, connection.next_event()).await.unwrap(),
        Err(Error::UnconfirmedClose)
    ));
    assert!(matches!(
        connection.disconnect().await,
        Err(Error::UnconfirmedClose)
    ));
    assert!(connection.next_event().await.unwrap().is_none());
    peer.await.unwrap();
}
