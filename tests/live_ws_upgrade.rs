#![allow(clippy::too_many_lines)] // Handshake matrices keep each complete exchange visible.

use futures::{SinkExt, StreamExt};
use oai_rt_rs::live::{
    ClientOptions, Error, ForkSessionConfig, HttpBodyIssue, LiveClient, SessionConfig,
};
use serde_json::json;
use std::{
    io::{self, Write},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::timeout,
};
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{
        Message,
        client::IntoClientRequest,
        handshake::{client::generate_request, derive_accept_key},
        protocol::Role,
    },
};

async fn request(tcp: &mut TcpStream) -> String {
    let mut bytes = Vec::new();
    while !bytes.ends_with(b"\r\n\r\n") {
        let mut byte = [0];
        tcp.read_exact(&mut byte).await.unwrap();
        bytes.push(byte[0]);
        assert!(bytes.len() < 8192);
    }
    String::from_utf8(bytes).unwrap()
}

fn accept_key(request: &str) -> String {
    let key = request
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("sec-websocket-key")
                .then(|| value.trim())
        })
        .unwrap();
    derive_accept_key(key.as_bytes())
}

fn client(listener: &TcpListener, secret: &str) -> LiveClient {
    LiveClient::with_options(
        secret,
        ClientOptions {
            base_url: format!("http://{}/v1/", listener.local_addr().unwrap())
                .parse()
                .unwrap(),
            request_timeout: Duration::from_secs(2),
            ..ClientOptions::default()
        },
    )
    .unwrap()
}

struct LogWriter(Arc<Mutex<Vec<u8>>>);

impl Write for LogWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn authenticated_primary_fork_and_sideband_never_log_bearer_at_trace() {
    const SECRET: &str = "SYNTHETIC_TRACE_SECRET_NEVER_USE_A_REAL_KEY";
    let logs = Arc::new(Mutex::new(Vec::new()));
    let writer = Arc::clone(&logs);
    tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_max_level(tracing::Level::TRACE)
        .with_writer(move || LogWriter(Arc::clone(&writer)))
        .try_init()
        .unwrap();
    // Prove the capturing logger sees the vulnerable dependency's raw-header TRACE.
    let mut control = "ws://localhost/control".into_client_request().unwrap();
    let mut header = reqwest::header::HeaderValue::from_str(&format!("Bearer {SECRET}")).unwrap();
    header.set_sensitive(true);
    control.headers_mut().insert("authorization", header);
    generate_request(control).unwrap();
    assert!(String::from_utf8_lossy(&logs.lock().unwrap()).contains(SECRET));
    logs.lock().unwrap().clear();

    for operation in 0..3 {
        for successful in [false, true] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let client = client(&listener, SECRET);
            let peer = tokio::spawn(async move {
                let (mut tcp, _) = listener.accept().await.unwrap();
                let request = request(&mut tcp).await;
                assert!(request.contains(SECRET));
                if !successful {
                    tcp.write_all(b"HTTP/1.1 429 Too Many Requests\r\nContent-Length: 4\r\nX-Request-Id: req-trace\r\nRetry-After: 3\r\n\r\ndeny").await.unwrap();
                    return;
                }
                let session = json!({"id":"new-session","model":"gpt-live-1","status":"active","expires_at":1});
                let started =
                    json!({"type":"session.started","event_id":"s","session":session}).to_string();
                let mut reply = format!("HTTP/1.1 101 Switching Protocols\r\nUpgrade: WebSocket\r\nConnection: keep-alive, uPgRaDe\r\nSec-WebSocket-Accept: {}\r\n\r\n", accept_key(&request)).into_bytes();
                if operation == 2 {
                    reply.extend([0x81, 126]);
                    reply.extend(u16::try_from(started.len()).unwrap().to_be_bytes());
                    reply.extend(started.as_bytes());
                }
                tcp.write_all(&reply).await.unwrap();
                let mut ws = WebSocketStream::from_raw_socket(tcp, Role::Server, None).await;
                if operation != 2 {
                    ws.next().await.unwrap().unwrap();
                    ws.send(Message::Text(started.into())).await.unwrap();
                }
                ws.next().await.unwrap().unwrap();
                ws.send(Message::Text(
                    json!({"type":"session.closed","event_id":"c","session":session,
                    "reason":"close_requested","usage":{"seconds":1}})
                    .to_string()
                    .into(),
                ))
                .await
                .unwrap();
            });
            let result = match operation {
                0 => client.connect(SessionConfig::default()).await,
                1 => client.fork("source", ForkSessionConfig::default()).await,
                _ => client.attach("existing").await,
            };
            if successful {
                result
                    .unwrap()
                    .close(Duration::from_secs(2), |_| Ok(()))
                    .await
                    .unwrap();
            } else {
                assert!(matches!(
                    result,
                    Err(Error::Http {
                        status: 429,
                        body_issue: None,
                        ..
                    })
                ));
            }
            peer.await.unwrap();
            let output = String::from_utf8_lossy(&logs.lock().unwrap()).into_owned();
            assert!(
                !output.contains(SECRET),
                "authenticated handshake leaked a synthetic bearer"
            );
        }
    }
}

#[tokio::test]
async fn upgrade_requires_accept_connection_upgrade_and_no_unoffered_protocols() {
    for (upgrade, connection, accept, extra, version) in [
        ("not-websocket", "Upgrade", true, "", "1.1"),
        ("websocket", "keep-alive", true, "", "1.1"),
        ("websocket", "Upgrade", false, "", "1.1"),
        (
            "websocket",
            "Upgrade",
            true,
            "Sec-WebSocket-Protocol: unoffered\r\n",
            "1.1",
        ),
        (
            "websocket",
            "Upgrade",
            true,
            "Sec-WebSocket-Extensions: permessage-deflate\r\n",
            "1.1",
        ),
        (
            "websocket",
            "Upgrade",
            true,
            "Upgrade: websocket\r\n",
            "1.1",
        ),
        (
            "websocket",
            "Upgrade",
            true,
            "Sec-WebSocket-Accept: duplicate\r\n",
            "1.1",
        ),
        ("websocket", ", Upgrade", true, "", "1.1"),
        ("websocket", "Upgrade", true, "", "1.0"),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = client(&listener, "synthetic");
        let peer = tokio::spawn(async move {
            let (mut tcp, _) = listener.accept().await.unwrap();
            let request = request(&mut tcp).await;
            let accept = if accept {
                accept_key(&request)
            } else {
                "invalid".into()
            };
            tcp.write_all(format!("HTTP/{version} 101 Switching Protocols\r\nUpgrade: {upgrade}\r\nConnection: {connection}\r\nSec-WebSocket-Accept: {accept}\r\n{extra}\r\n").as_bytes()).await.unwrap();
            let mut byte = [0];
            if let Ok(Ok(read)) = timeout(Duration::from_millis(100), tcp.read(&mut byte)).await {
                assert_eq!(read, 0, "no session.start may follow an invalid handshake");
            }
        });
        assert!(client.connect(SessionConfig::default()).await.is_err());
        peer.await.unwrap();
    }
}

#[tokio::test]
async fn redirects_and_invalid_lengths_never_confirm_a_complete_rejection_body() {
    for framing in [
        "Content-Length: +0\r\n",
        "Content-Length: +7\r\n",
        "Content-Length: 0\r\nContent-Length: 7\r\n",
        "Content-Length: 0, 7\r\n",
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = client(&listener, "synthetic");
        let peer = tokio::spawn(async move {
            let (mut tcp, _) = listener.accept().await.unwrap();
            request(&mut tcp).await;
            tcp.write_all(format!("HTTP/1.1 429 Too Many Requests\r\n{framing}\r\n").as_bytes())
                .await
                .unwrap();
        });
        let result = client.attach("existing").await;
        assert!(result.is_err());
        assert!(!matches!(
            result,
            Err(Error::Http {
                body_issue: None,
                ..
            })
        ));
        peer.await.unwrap();
    }
    let redirect = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = client(&listener, "synthetic");
    let location = format!("http://{}/trap", redirect.local_addr().unwrap());
    let peer = tokio::spawn(async move {
        let (mut tcp, _) = listener.accept().await.unwrap();
        request(&mut tcp).await;
        tcp.write_all(
            format!("HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\n\r\n")
                .as_bytes(),
        )
        .await
        .unwrap();
    });
    assert!(matches!(
        client.attach("existing").await,
        Err(Error::Http { status: 302, .. })
    ));
    peer.await.unwrap();
    assert!(
        timeout(Duration::from_millis(50), redirect.accept())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn rejection_body_deadline_keeps_http_status_headers_and_bounded_prefix() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = LiveClient::with_options(
        "synthetic",
        ClientOptions {
            base_url: format!("http://{}/v1/", listener.local_addr().unwrap())
                .parse()
                .unwrap(),
            request_timeout: Duration::from_millis(100),
            ..ClientOptions::default()
        },
    )
    .unwrap();
    let peer = tokio::spawn(async move {
        let (mut tcp, _) = listener.accept().await.unwrap();
        request(&mut tcp).await;
        tcp.write_all(b"HTTP/1.1 429 Too Many Requests\r\nContent-Length: 100\r\nX-Request-Id: req-slow\r\nRetry-After: 4\r\n\r\nprefix").await.unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
    });
    let error = client.attach("existing").await.err().unwrap();
    let Error::Http {
        status,
        headers,
        body,
        request_id,
        retry_after,
        body_issue,
        ..
    } = error
    else {
        panic!("body timeout must not erase known HTTP rejection evidence");
    };
    assert_eq!(status, 429);
    assert_eq!(headers["content-length"], "100");
    assert_eq!(request_id.as_deref(), Some("req-slow"));
    assert_eq!(retry_after.as_deref(), Some("4"));
    assert_eq!(body, b"prefix");
    assert_eq!(body_issue, Some(HttpBodyIssue::ReadFailed));
    peer.await.unwrap();
}
