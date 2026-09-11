use axum::{
    Router,
    body::{Body, to_bytes},
    extract::{Request, State},
    response::Response,
    routing::any,
};
use oai_rt_rs::live::{
    AudioConfig, AudioFormat, ClientOptions, Codec, CreateRequest, CreateResponse, Error,
    LiveClient, SessionConfig, WebRtcTransport, decode_request,
};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};
use tokio::{net::TcpListener, task::JoinHandle};

struct Reply {
    status: u16,
    content_type: &'static str,
    body: Vec<u8>,
}

struct Captured {
    method: String,
    path: String,
    body: Vec<u8>,
}

struct StateData {
    replies: Mutex<VecDeque<Reply>>,
    captured: Mutex<Vec<Captured>>,
}

struct Mock {
    client: LiveClient,
    data: Arc<StateData>,
    task: JoinHandle<()>,
}

impl Drop for Mock {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn handle(State(data): State<Arc<StateData>>, request: Request) -> Response {
    assert_eq!(
        request.headers()["authorization"],
        "Bearer synthetic-test-key"
    );
    assert_eq!(request.headers()["openai-project"], "test-project");
    assert!(!request.headers().contains_key("openai-alpha"));
    assert!(!request.headers().contains_key("openai-beta"));
    let method = request.method().to_string();
    let path = request.uri().to_string();
    let body = to_bytes(request.into_body(), 1024 * 1024)
        .await
        .unwrap()
        .to_vec();
    data.captured
        .lock()
        .unwrap()
        .push(Captured { method, path, body });
    let reply = data
        .replies
        .lock()
        .unwrap()
        .pop_front()
        .expect("no automatic HTTP retry");
    Response::builder()
        .status(reply.status)
        .header("content-type", reply.content_type)
        .header("x-request-id", "req-test")
        .header("retry-after", "3")
        .header("location", "/unexpected-redirect")
        .body(Body::from(reply.body))
        .unwrap()
}

async fn mock(replies: Vec<Reply>, limit: usize) -> Mock {
    let data = Arc::new(StateData {
        replies: Mutex::new(replies.into()),
        captured: Mutex::new(Vec::new()),
    });
    let app = Router::new().fallback(any(handle)).with_state(data.clone());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = LiveClient::with_options(
        "synthetic-test-key",
        ClientOptions {
            project: Some("test-project".into()),
            base_url: format!("http://{}/v1/", listener.local_addr().unwrap())
                .parse()
                .unwrap(),
            codec: Codec {
                max_event_bytes: limit,
            },
            ..ClientOptions::default()
        },
    )
    .unwrap();
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    Mock { client, data, task }
}

#[tokio::test]
async fn every_call_control_uses_exact_public_path_body_and_empty_success() {
    use oai_rt_rs::live::{AcceptRequest, ReferRequest, RejectRequest, SipAcceptSession};
    let mock = mock(
        (0..4)
            .map(|_| Reply {
                status: 200,
                content_type: "text/plain",
                body: vec![],
            })
            .collect(),
        4096,
    )
    .await;
    mock.client
        .accept_call(
            "session_id",
            &AcceptRequest {
                session: SipAcceptSession::new(SessionConfig::default()),
            },
        )
        .await
        .unwrap();
    mock.client
        .reject_call("session_id", RejectRequest { status_code: 486 })
        .await
        .unwrap();
    mock.client
        .refer_call(
            "session_id",
            &ReferRequest {
                target_uri: "sip:agent@example.com".into(),
            },
        )
        .await
        .unwrap();
    mock.client.hangup("session_id").await.unwrap();
    let captured = mock.data.captured.lock().unwrap();
    assert_eq!(captured.len(), 4);
    for (index, action) in ["accept", "reject", "refer", "hangup"].iter().enumerate() {
        assert_eq!(captured[index].method, "POST");
        assert_eq!(
            captured[index].path,
            format!("/v1/live/sessions/session_id/{action}")
        );
    }
    assert_eq!(
        serde_json::from_slice::<Value>(&captured[0].body).unwrap(),
        json!({"session":{"type":"live","model":"gpt-live-1"}})
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&captured[1].body).unwrap(),
        json!({"status_code":486})
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&captured[2].body).unwrap(),
        json!({"target_uri":"sip:agent@example.com"})
    );
    assert!(captured[3].body.is_empty());
    drop(captured);
}

#[tokio::test]
async fn call_control_does_not_accept_conflicts_wrong_status_or_nonempty_success() {
    use oai_rt_rs::live::RejectRequest;
    let mock = mock(
        vec![
            json_reply(409, json!({"error":{"code":"decision_already_made"}})),
            Reply {
                status: 204,
                content_type: "text/plain",
                body: vec![],
            },
            json_reply(200, json!({})),
        ],
        4096,
    )
    .await;
    assert!(matches!(
        mock.client
            .reject_call("s", RejectRequest { status_code: 486 })
            .await,
        Err(Error::Http { status: 409, .. })
    ));
    assert!(matches!(
        mock.client.hangup("s").await,
        Err(Error::Http { status: 204, .. })
    ));
    assert!(matches!(
        mock.client.hangup("s").await,
        Err(Error::Invalid(_))
    ));
    let before = mock.data.captured.lock().unwrap().len();
    assert!(
        mock.client
            .reject_call("s", RejectRequest { status_code: 700 })
            .await
            .is_err()
    );
    assert!(mock.client.hangup("..").await.is_err());
    assert_eq!(mock.data.captured.lock().unwrap().len(), before);
}

#[tokio::test]
async fn http_fork_uses_new_sdp_and_distinguishes_omission_from_empty_overrides() {
    use oai_rt_rs::live::{ForkAudioConfig, ForkRequest, ForkSessionConfig};
    let reply = || {
        json_reply(
            201,
            json!({"session":{"id":"new_fork"},"transport":{"type":"webrtc","sdp":"answer"}}),
        )
    };
    let mock = mock(vec![reply(), reply()], 4096).await;
    let mut request = ForkRequest {
        transport: WebRtcTransport::WebRtc {
            sdp: "offer".into(),
        },
        session: None,
    };
    let fork = mock.client.fork_webrtc("stored", &request).await.unwrap();
    assert_eq!(fork.session.id, "new_fork");
    request.session = Some(ForkSessionConfig::default());
    mock.client.fork_webrtc("stored", &request).await.unwrap();
    request.session = Some(ForkSessionConfig {
        audio: Some(ForkAudioConfig::default()),
        ..ForkSessionConfig::default()
    });
    assert!(mock.client.fork_webrtc("stored", &request).await.is_err());
    let captured = mock.data.captured.lock().unwrap();
    assert_eq!(captured.len(), 2);
    assert_eq!(captured[0].path, "/v1/live/sessions/stored/fork");
    assert_eq!(
        serde_json::from_slice::<Value>(&captured[0].body).unwrap(),
        json!({"transport":{"type":"webrtc","sdp":"offer"}})
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&captured[1].body).unwrap(),
        json!({"session":{},"transport":{"type":"webrtc","sdp":"offer"}})
    );
    drop(captured);
}

#[tokio::test]
async fn endpoint_identifiers_are_encoded_as_one_path_segment() {
    let mock = mock(
        vec![Reply {
            status: 200,
            content_type: "text/plain",
            body: vec![],
        }],
        4096,
    )
    .await;
    mock.client.hangup("opaque/child?query").await.unwrap();
    assert_eq!(
        mock.data.captured.lock().unwrap()[0].path,
        "/v1/live/sessions/opaque%2Fchild%3Fquery/hangup"
    );
}

fn create_request() -> CreateRequest {
    CreateRequest {
        session: SessionConfig::default(),
        transport: WebRtcTransport::WebRtc {
            sdp: "v=0\r\n".into(),
        },
    }
}

#[allow(clippy::needless_pass_by_value)]
fn json_reply(status: u16, body: Value) -> Reply {
    Reply {
        status,
        content_type: "application/json",
        body: body.to_string().into_bytes(),
    }
}

#[test]
fn create_representations_require_only_id_in_the_created_session() {
    let request =
        json!({"session":{"model":"gpt-live-1"},"transport":{"type":"webrtc","sdp":"v=0\r\n"}});
    assert_eq!(serde_json::to_value(create_request()).unwrap(), request);
    let response =
        json!({"session":{"id":"live_new"},"transport":{"type":"webrtc","sdp":"answer"}});
    let typed: CreateResponse = serde_json::from_value(response.clone()).unwrap();
    assert_eq!(serde_json::to_value(typed).unwrap(), response);
    for value in [
        json!({"session":{"model":"gpt-live-1","type":"live"},"transport":{"type":"webrtc","sdp":"offer"}}),
        json!({"session":{"model":"gpt-live-1"},"transport":{"type":"webrtc","sdp":"offer","extra":true}}),
        json!({"session":{"model":"gpt-live-1"},"transport":null}),
    ] {
        assert!(decode_request::<CreateRequest>(&value.to_string()).is_err());
    }
}

#[tokio::test]
async fn public_create_uses_authenticated_json_without_private_bootstrap() {
    let mock = mock(
        vec![json_reply(
            201,
            json!({"session":{"id":"live_new"},"transport":{"type":"webrtc","sdp":"answer"}}),
        )],
        4096,
    )
    .await;
    let response = mock.client.create_webrtc(&create_request()).await.unwrap();
    assert_eq!(response.session.id, "live_new");
    assert_eq!(response.transport.sdp(), "answer");
    let captured = mock.data.captured.lock().unwrap();
    assert_eq!(captured.len(), 1);
    assert_eq!(captured[0].method, "POST");
    assert_eq!(captured[0].path, "/v1/live/sessions");
    assert_eq!(
        serde_json::from_slice::<Value>(&captured[0].body).unwrap(),
        serde_json::to_value(create_request()).unwrap()
    );
    drop(captured);
}

#[tokio::test]
async fn webrtc_rejects_manual_audio_format_before_network_io() {
    let mock = mock(vec![], 4096).await;
    let mut request = create_request();
    request.session.audio = Some(AudioConfig {
        format: Some(AudioFormat::default()),
        output: None,
    });
    assert!(matches!(
        mock.client.create_webrtc(&request).await,
        Err(Error::Invalid(_))
    ));
    assert!(mock.data.captured.lock().unwrap().is_empty());
}

#[tokio::test]
async fn error_bodies_and_headers_are_explicit_redacted_and_never_retried() {
    for status in [200, 307, 401, 429, 500] {
        let body = b"synthetic-private-error-body".to_vec();
        let mock = mock(
            vec![Reply {
                status,
                content_type: "text/plain",
                body: body.clone(),
            }],
            4096,
        )
        .await;
        let error = mock
            .client
            .create_webrtc(&create_request())
            .await
            .unwrap_err();
        assert!(!format!("{error:?}").contains("synthetic-private"));
        match error {
            Error::Http {
                status: actual,
                body: actual_body,
                request_id,
                retry_after,
                content_type,
                body_issue,
                ..
            } => {
                assert_eq!(actual, status);
                assert_eq!(actual_body, body);
                assert_eq!(request_id.as_deref(), Some("req-test"));
                assert_eq!(retry_after.as_deref(), Some("3"));
                assert_eq!(content_type.as_deref(), Some("text/plain"));
                assert_eq!(body_issue, None);
            }
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(mock.data.captured.lock().unwrap().len(), 1);
    }
}

#[tokio::test]
async fn malformed_successes_and_oversized_responses_are_not_success_shaped() {
    for reply in [
        Reply {
            status: 201,
            content_type: "text/plain",
            body: b"raw private SDP is not public JSON".to_vec(),
        },
        Reply {
            status: 201,
            content_type: "application/json",
            body: b"{malformed".to_vec(),
        },
        json_reply(
            201,
            json!({"session":{},"transport":{"type":"webrtc","sdp":"answer"}}),
        ),
        json_reply(
            201,
            json!({"session":{"id":null},"transport":{"type":"webrtc","sdp":"answer"}}),
        ),
    ] {
        let mock = mock(vec![reply], 4096).await;
        assert!(mock.client.create_webrtc(&create_request()).await.is_err());
    }
    let mock = mock(
        vec![Reply {
            status: 500,
            content_type: "text/plain",
            body: vec![0; 256],
        }],
        128,
    )
    .await;
    let error = mock
        .client
        .create_webrtc(&create_request())
        .await
        .unwrap_err();
    assert!(
        matches!(error,Error::Http {status:500,body_issue:Some(oai_rt_rs::live::HttpBodyIssue::Truncated),ref body,ref request_id,ref retry_after,..}
        if body.len()==128 && request_id.as_deref()==Some("req-test") && retry_after.as_deref()==Some("3"))
    );
}

#[tokio::test]
async fn interrupted_error_body_preserves_status_headers_and_available_prefix() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = LiveClient::with_options(
        "synthetic",
        ClientOptions {
            base_url: format!("http://{}/v1/", listener.local_addr().unwrap())
                .parse()
                .unwrap(),
            ..ClientOptions::default()
        },
    )
    .unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0; 4096];
        socket.read(&mut request).await.unwrap();
        socket.write_all(b"HTTP/1.1 429 Too Many Requests\r\nContent-Length: 20\r\nContent-Type: text/plain\r\nRetry-After: 7\r\nX-Request-Id: req-stream\r\nConnection: close\r\n\r\npartial").await.unwrap();
        socket.shutdown().await.unwrap();
    });
    let error = client.create_webrtc(&create_request()).await.unwrap_err();
    match error {
        Error::Http {
            status,
            headers,
            body,
            request_id,
            retry_after,
            body_issue,
            ..
        } => {
            assert_eq!(status, 429);
            assert_eq!(headers["retry-after"], "7");
            assert_eq!(request_id.as_deref(), Some("req-stream"));
            assert_eq!(retry_after.as_deref(), Some("7"));
            assert_eq!(body_issue, Some(oai_rt_rs::live::HttpBodyIssue::ReadFailed));
            assert!(b"partial".starts_with(&body));
        }
        other => panic!("lost known HTTP metadata: {other:?}"),
    }
    server.await.unwrap();
}

fn wav() -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&40_u32.to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&24000_u32.to_le_bytes());
    bytes.extend_from_slice(&96000_u32.to_le_bytes());
    bytes.extend_from_slice(&4_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&4_u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    bytes
}

#[tokio::test]
async fn recording_download_streams_bytes_and_enforces_caller_limit() {
    let bytes = wav();
    let mock = mock(
        vec![
            Reply {
                status: 200,
                content_type: "audio/wav",
                body: bytes.clone(),
            },
            Reply {
                status: 200,
                content_type: "audio/wav; charset=binary",
                body: bytes.clone(),
            },
            Reply {
                status: 200,
                content_type: "text/plain",
                body: bytes.clone(),
            },
            json_reply(404, json!({"error":{"code":"not_found"}})),
        ],
        4096,
    )
    .await;
    assert!(mock.client.download_content("stored").await.is_err());
    assert!(mock.client.download_content("live_").await.is_err());
    assert!(mock.client.download_content("live_bad/path").await.is_err());
    let content = mock.client.download_content("live_stored").await.unwrap();
    assert_eq!(content.metadata.content_length, Some(48));
    assert_eq!(content.metadata.request_id.as_deref(), Some("req-test"));
    assert_eq!(content.read_all(48).await.unwrap(), bytes);
    assert!(matches!(
        mock.client
            .download_content("live_stored")
            .await
            .unwrap()
            .read_all(47)
            .await,
        Err(Error::Invalid(_))
    ));
    assert!(matches!(
        mock.client.download_content("live_stored").await,
        Err(Error::Invalid(_))
    ));
    assert!(matches!(
        mock.client.download_content("live_stored").await,
        Err(Error::Http { status: 404, .. })
    ));
    for captured in mock.data.captured.lock().unwrap().iter() {
        assert_eq!(captured.method, "GET");
        assert_eq!(captured.path, "/v1/live/sessions/live_stored/content");
        assert!(captured.body.is_empty());
    }
}
