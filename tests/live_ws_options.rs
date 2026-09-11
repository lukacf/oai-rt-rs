use futures::{SinkExt, StreamExt};
use oai_rt_rs::live::{ClientOptions, LiveClient, ServerEvent, SidebandOptions};
use serde_json::json;
use tokio::net::TcpListener;
use tokio_tungstenite::{
    accept_hdr_async,
    tungstenite::{
        Message,
        handshake::server::{Request, Response},
    },
};

#[tokio::test]
#[allow(clippy::result_large_err)]
async fn sideband_graceful_close_query_preserves_absence_and_false() {
    for value in [None, Some(false), Some(true)] {
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
        let peer = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut ws = accept_hdr_async(tcp, move |request: &Request, response: Response| {
                assert_eq!(request.uri().path(), "/v1/live/sessions/live_s/attach");
                let expected = value.map(|value| format!("graceful_close={value}"));
                assert_eq!(request.uri().query(), expected.as_deref());
                Ok(response)
            })
            .await
            .unwrap();
            ws.send(Message::Text(json!({
                "type":"session.closed","event_id":"e","reason":"remote_hangup",
                "session":{"id":"live_s","model":"gpt-live-1","status":"active","expires_at":10},
                "usage":{"seconds":1}
            }).to_string().into())).await.unwrap();
            let _ = ws.next().await;
        });
        let mut connection = client
            .attach_with_options(
                "live_s",
                SidebandOptions {
                    graceful_close: value,
                },
            )
            .await
            .unwrap();
        assert!(matches!(
            connection.next_event().await.unwrap().unwrap().event,
            ServerEvent::Closed { .. }
        ));
        drop(connection);
        peer.await.unwrap();
    }
}
