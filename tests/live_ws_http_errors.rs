use oai_rt_rs::live::{ClientOptions, Error, HttpBodyIssue, LiveClient, SessionConfig};
use std::time::Duration;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::oneshot,
    time::timeout,
};

#[tokio::test]
async fn split_upgrade_headers_do_not_claim_a_complete_error_body() {
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
    let (continue_tx, continue_rx) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0; 4096];
        assert!(socket.read(&mut request).await.unwrap() > 0);
        socket.write_all(b"HTTP/1.1 429 Too Many Requests\r\nContent-Length: 7\r\nRetry-After: 4\r\nX-Request-Id: req-upgrade\r\n\r\n").await.unwrap();
        continue_rx.await.unwrap();
        let _ = socket.write_all(b"payload").await;
        assert!(
            timeout(Duration::from_millis(30), listener.accept())
                .await
                .is_err()
        );
    });
    let error = client
        .connect(SessionConfig::default())
        .await
        .err()
        .unwrap();
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
            assert_eq!(headers["content-length"], "7");
            assert_eq!(request_id.as_deref(), Some("req-upgrade"));
            assert_eq!(retry_after.as_deref(), Some("4"));
            assert!(body.is_empty());
            assert_eq!(body_issue, Some(HttpBodyIssue::Unconfirmed));
        }
        other => panic!("lost upgrade HTTP metadata: {other:?}"),
    }
    continue_tx.send(()).unwrap();
    server.await.unwrap();
}

#[tokio::test]
async fn unknown_and_chunked_upgrade_framing_remain_unconfirmed() {
    for framing in ["", "Transfer-Encoding: chunked\r\n"] {
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
            assert!(socket.read(&mut request).await.unwrap() > 0);
            socket.write_all(format!("HTTP/1.1 403 Forbidden\r\n{framing}Connection: close\r\n\r\n7\r\npayload\r\n0\r\n\r\n").as_bytes()).await.unwrap();
        });
        let error = client
            .connect(SessionConfig::default())
            .await
            .err()
            .unwrap();
        assert!(matches!(
            error,
            Error::Http {
                status: 403,
                body_issue: Some(HttpBodyIssue::Unconfirmed),
                ..
            }
        ));
        server.await.unwrap();
    }
}
