use crate::error::Result;
use crate::protocol::models::{DEFAULT_MODEL, GPT_REALTIME_TRANSLATE};
use reqwest::header::HeaderValue;
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};
use url::Url;

#[derive(Debug)]
pub struct WsStream(WebSocketStream<MaybeTlsStream<TcpStream>>);

impl WsStream {
    pub(crate) const fn new(stream: WebSocketStream<MaybeTlsStream<TcpStream>>) -> Self {
        Self(stream)
    }
}

impl futures::Stream for WsStream {
    type Item = std::result::Result<
        tokio_tungstenite::tungstenite::Message,
        tokio_tungstenite::tungstenite::Error,
    >;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        std::pin::Pin::new(&mut self.0).poll_next(cx)
    }
}

impl futures::Sink<tokio_tungstenite::tungstenite::Message> for WsStream {
    type Error = tokio_tungstenite::tungstenite::Error;

    fn poll_ready(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::result::Result<(), Self::Error>> {
        std::pin::Pin::new(&mut self.0).poll_ready(cx)
    }

    fn start_send(
        mut self: std::pin::Pin<&mut Self>,
        item: tokio_tungstenite::tungstenite::Message,
    ) -> std::result::Result<(), Self::Error> {
        std::pin::Pin::new(&mut self.0).start_send(item)
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::result::Result<(), Self::Error>> {
        std::pin::Pin::new(&mut self.0).poll_flush(cx)
    }

    fn poll_close(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::result::Result<(), Self::Error>> {
        std::pin::Pin::new(&mut self.0).poll_close(cx)
    }
}

const WS_BASE_URL: &str = "wss://api.openai.com/v1/realtime";
const WS_TRANSLATIONS_URL: &str = "wss://api.openai.com/v1/realtime/translations";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WsConnectTarget {
    Realtime,
    Transcription,
    Translation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WsConnectOptions<'a> {
    pub model: Option<&'a str>,
    pub call_id: Option<&'a str>,
    pub safety_identifier: Option<&'a str>,
    pub target: WsConnectTarget,
}

impl Default for WsConnectOptions<'_> {
    fn default() -> Self {
        Self {
            model: None,
            call_id: None,
            safety_identifier: None,
            target: WsConnectTarget::Realtime,
        }
    }
}

/// Establish a WebSocket connection to the Realtime API.
///
/// # Errors
/// Returns an error if the handshake fails.
pub async fn connect(
    api_key: &str,
    model: Option<&str>,
    call_id: Option<&str>,
) -> Result<WsStream> {
    connect_with_options(
        api_key,
        WsConnectOptions {
            model,
            call_id,
            ..WsConnectOptions::default()
        },
    )
    .await
}

/// Establish a WebSocket connection with explicit target and headers.
///
/// # Errors
/// Returns an error if the handshake fails.
pub async fn connect_with_options(
    api_key: &str,
    options: WsConnectOptions<'_>,
) -> Result<WsStream> {
    let url = ws_url_for_options(&options)?;
    let auth_header = HeaderValue::from_str(&format!("Bearer {api_key}"))?;

    let mut req = tokio_tungstenite::tungstenite::client::IntoClientRequest::into_client_request(
        url.as_str(),
    )?;
    let h = req.headers_mut();
    h.insert(reqwest::header::AUTHORIZATION, auth_header);
    if let Some(identifier) = options.safety_identifier {
        h.insert(
            "OpenAI-Safety-Identifier",
            HeaderValue::from_str(identifier)?,
        );
    }
    let (ws_stream, _) = connect_async(req).await?;

    tracing::info!("Connected to OpenAI Realtime");

    Ok(WsStream::new(ws_stream))
}

/// Build the WebSocket URL for testable connection-target routing.
///
/// # Errors
/// Returns an error if the base URL is invalid.
#[allow(clippy::result_large_err)]
pub fn ws_url_for_options(options: &WsConnectOptions<'_>) -> Result<Url> {
    let mut url = match options.target {
        WsConnectTarget::Realtime | WsConnectTarget::Transcription => Url::parse(WS_BASE_URL)?,
        WsConnectTarget::Translation => Url::parse(WS_TRANSLATIONS_URL)?,
    };

    {
        let mut query = url.query_pairs_mut();
        if options.target == WsConnectTarget::Transcription {
            query.append_pair("intent", "transcription");
        } else if let Some(cid) = options.call_id {
            query.append_pair("call_id", cid);
        } else {
            let default_model = match options.target {
                WsConnectTarget::Realtime | WsConnectTarget::Transcription => DEFAULT_MODEL,
                WsConnectTarget::Translation => GPT_REALTIME_TRANSLATE,
            };
            query.append_pair("model", options.model.unwrap_or(default_model));
        }
    }

    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translation_url_uses_dedicated_endpoint_and_model() {
        let url = ws_url_for_options(&WsConnectOptions {
            target: WsConnectTarget::Translation,
            ..WsConnectOptions::default()
        })
        .expect("url");

        assert_eq!(url.path(), "/v1/realtime/translations");
        assert_eq!(url.query(), Some("model=gpt-realtime-translate"));
    }

    #[test]
    fn transcription_url_uses_intent() {
        let url = ws_url_for_options(&WsConnectOptions {
            target: WsConnectTarget::Transcription,
            ..WsConnectOptions::default()
        })
        .expect("url");

        assert_eq!(url.path(), "/v1/realtime");
        assert_eq!(url.query(), Some("intent=transcription"));
    }

    #[test]
    fn realtime_url_keeps_model_or_call_id_routing() {
        let url = ws_url_for_options(&WsConnectOptions {
            model: Some("gpt-realtime-2"),
            ..WsConnectOptions::default()
        })
        .expect("model url");
        assert_eq!(url.path(), "/v1/realtime");
        assert_eq!(url.query(), Some("model=gpt-realtime-2"));

        let url = ws_url_for_options(&WsConnectOptions {
            call_id: Some("call_123"),
            ..WsConnectOptions::default()
        })
        .expect("call url");
        assert_eq!(url.path(), "/v1/realtime");
        assert_eq!(url.query(), Some("call_id=call_123"));
    }
}
