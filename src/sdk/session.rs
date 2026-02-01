use crate::protocol::client_events::ClientEvent;
use crate::protocol::models::{ContentPart, Item, SessionConfig, SessionUpdate};
use crate::protocol::server_events::ServerEvent;
use crate::transport::ws::ProtocolVersion;
use crate::{Error, Result};

use super::handlers::EventHandlers;
use super::tools::{ToolCall, ToolRegistry, ToolResult};
use super::transport::Transport;
use std::collections::HashMap;
use tokio::sync::{mpsc, oneshot};

#[derive(Clone)]
pub struct SessionHandle {
    sender: mpsc::Sender<Command>,
}

pub struct Session {
    sender: mpsc::Sender<Command>,
    text_rx: mpsc::Receiver<String>,
}

impl Session {
    #[must_use]
    pub fn handle(&self) -> SessionHandle {
        SessionHandle {
            sender: self.sender.clone(),
        }
    }

    /// Send a single user text message and return immediately.
    ///
    /// # Errors
    /// Returns an error if the SDK is not fully initialized or the send fails.
    pub async fn say(&self, text: &str) -> Result<()> {
        let item = Item::Message {
            id: None,
            status: None,
            role: crate::protocol::models::Role::User,
            content: vec![ContentPart::InputText { text: text.to_string() }],
        };

        let event = ClientEvent::ConversationItemCreate {
            event_id: None,
            previous_item_id: None,
            item: Box::new(item),
        };

        self.send_event(event).await
    }

    /// Await the next completed text response, if any.
    ///
    /// # Errors
    /// Returns an error if the SDK is not fully initialized or the stream fails.
    pub async fn next_text(&mut self) -> Result<Option<String>> {
        Ok(self.text_rx.recv().await)
    }

    /// Send a raw protocol event.
    ///
    /// # Errors
    /// Returns an error if the SDK is not fully initialized or the send fails.
    pub async fn send_raw(&self, event: ClientEvent) -> Result<()> {
        self.send_event(event).await
    }

    /// Dispatch a tool call to the registry.
    ///
    /// # Errors
    /// Returns an error if the tool is missing or execution fails.
    pub async fn run_tool(&self, call: ToolCall) -> Result<ToolResult> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(Command::RunTool { call, respond: tx })
            .await
            .map_err(|_| Error::ConnectionClosed)?;
        rx.await.map_err(|_| Error::ConnectionClosed)?
    }

    /// Apply a session update.
    ///
    /// # Errors
    /// Returns an error if the SDK is not fully initialized or the update fails.
    pub async fn update_session(&self, update: SessionUpdate) -> Result<()> {
        let event = ClientEvent::SessionUpdate {
            event_id: None,
            session: Box::new(update),
        };
        self.send_event(event).await
    }

    async fn send_event(&self, event: ClientEvent) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(Command::SendWithResponse { event, respond: tx })
            .await
            .map_err(|_| Error::ConnectionClosed)?;
        rx.await.map_err(|_| Error::ConnectionClosed)??;
        Ok(())
    }

    fn from_transport(
        mut transport: Box<dyn Transport>,
        handlers: EventHandlers,
        tools: ToolRegistry,
    ) -> Self {
        let (cmd_tx, mut cmd_rx) = mpsc::channel::<Command>(64);
        let (text_tx, text_rx) = mpsc::channel::<String>(64);

        tokio::spawn(async move {
            let mut buffers: HashMap<(String, u32), String> = HashMap::new();
            loop {
                tokio::select! {
                    cmd = cmd_rx.recv() => {
                        match cmd {
                            Some(Command::SendWithResponse { event, respond }) => {
                                let result = transport.send(event).await;
                                let _ = respond.send(result);
                            }
                            Some(Command::RunTool { call, respond }) => {
                                let result = tools.dispatch(call).await;
                                let _ = respond.send(result);
                            }
                            None => break,
                        }
                    }
                    event = transport.next_event() => {
                        match event {
                            Ok(Some(evt)) => {
                                if let Some(handler) = &handlers.on_raw_event {
                                    let _ = handler(evt.clone()).await;
                                }

                                match evt {
                                    ServerEvent::ResponseOutputTextDelta { item_id, content_index, delta, .. } => {
                                        let key = (item_id, content_index);
                                        let entry = buffers.entry(key).or_default();
                                        entry.push_str(&delta);
                                    }
                                    ServerEvent::ResponseOutputTextDone { item_id, content_index, text, .. } => {
                                        let key = (item_id, content_index);
                                        buffers.remove(&key);
                                        let _ = text_tx.send(text.clone()).await;
                                        if let Some(handler) = &handlers.on_text {
                                            let _ = handler(text).await;
                                        }
                                    }
                                    ServerEvent::ResponseFunctionCallArgumentsDone { call_id, name, arguments, .. } => {
                                        let arguments = serde_json::from_str(&arguments)
                                            .unwrap_or(serde_json::Value::String(arguments));
                                        let call = ToolCall { name, call_id: call_id.clone(), arguments };

                                        let result = if let Some(handler) = &handlers.on_tool_call {
                                            handler(call).await
                                        } else {
                                            tools.dispatch(call).await
                                        };

                                        if let Ok(tool_result) = result {
                                            let output = serde_json::to_string(&tool_result.output)
                                                .unwrap_or_else(|_| String::new());
                                            let item = Item::FunctionCallOutput {
                                                id: None,
                                                call_id: tool_result.call_id,
                                                output,
                                            };
                                            let event = ClientEvent::ConversationItemCreate {
                                                event_id: None,
                                                previous_item_id: None,
                                                item: Box::new(item),
                                            };
                                            let _ = transport.send(event).await;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            Ok(None) | Err(_) => break,
                        }
                    }
                }
            }
        });

        Self {
            sender: cmd_tx,
            text_rx,
        }
    }
}

impl SessionHandle {
    /// Send a raw protocol event.
    ///
    /// # Errors
    /// Returns an error if the send fails.
    pub async fn send_raw(&self, event: ClientEvent) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(Command::SendWithResponse { event, respond: tx })
            .await
            .map_err(|_| Error::ConnectionClosed)?;
        rx.await.map_err(|_| Error::ConnectionClosed)??;
        Ok(())
    }
}

enum Command {
    SendWithResponse { event: ClientEvent, respond: oneshot::Sender<Result<()>> },
    RunTool { call: ToolCall, respond: oneshot::Sender<Result<ToolResult>> },
}

#[allow(dead_code)]
pub(super) struct SessionConfigSnapshot {
    pub api_key: String,
    pub model: Option<String>,
    pub session: SessionConfig,
    pub handlers: EventHandlers,
    pub tools: ToolRegistry,
    pub protocol: ProtocolVersion,
}

impl SessionConfigSnapshot {
    /// Connect via WebSocket.
    ///
    /// # Errors
    /// Returns an error if the connection fails.
    pub async fn connect_ws(self) -> Result<Session> {
        let client = crate::RealtimeClient::connect_with_version(
            &self.api_key,
            self.model.as_deref(),
            None,
            self.protocol,
        )
        .await?;

        let transport = Box::new(WsTransport { client });
        Ok(Session::from_transport(transport, self.handlers, self.tools))
    }
}

struct WsTransport {
    client: crate::RealtimeClient,
}

impl Transport for WsTransport {
    fn send(&mut self, event: ClientEvent) -> super::transport::BoxFuture<'_, Result<()>> {
        Box::pin(async move { self.client.send(event).await })
    }

    fn next_event(&mut self) -> super::transport::BoxFuture<'_, Result<Option<ServerEvent>>> {
        Box::pin(async move { self.client.next_event().await })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::server_events::ServerEvent;
    use tokio::sync::mpsc;

    struct MockTransport {
        incoming: mpsc::Receiver<ServerEvent>,
        outgoing: mpsc::Sender<ClientEvent>,
    }

    impl Transport for MockTransport {
        fn send(&mut self, event: ClientEvent) -> super::super::transport::BoxFuture<'_, Result<()>> {
            let outgoing = self.outgoing.clone();
            Box::pin(async move {
                outgoing.send(event).await.map_err(|_| Error::ConnectionClosed)?;
                Ok(())
            })
        }

        fn next_event(&mut self) -> super::super::transport::BoxFuture<'_, Result<Option<ServerEvent>>> {
            Box::pin(async move { Ok(self.incoming.recv().await) })
        }
    }

    #[tokio::test]
    async fn tool_call_sends_output() {
        let (event_tx, event_rx) = mpsc::channel(8);
        let (out_tx, mut out_rx) = mpsc::channel(8);
        let transport = Box::new(MockTransport { incoming: event_rx, outgoing: out_tx });

        let mut tools = ToolRegistry::new();
        tools.tool("echo", |args: serde_json::Value| async move { Ok(args) });

        let session = Session::from_transport(transport, EventHandlers::new(), tools);

        let evt = ServerEvent::ResponseFunctionCallArgumentsDone {
            event_id: "evt_1".to_string(),
            response_id: "resp_1".to_string(),
            item_id: "item_1".to_string(),
            output_index: 0,
            call_id: "call_1".to_string(),
            name: "echo".to_string(),
            arguments: r#"{"hello":"world"}"#.to_string(),
        };

        event_tx.send(evt).await.unwrap();

        let sent = tokio::time::timeout(std::time::Duration::from_secs(1), out_rx.recv())
            .await
            .unwrap()
            .unwrap();

        match sent {
            ClientEvent::ConversationItemCreate { item, .. } => match *item {
                Item::FunctionCallOutput { call_id, output, .. } => {
                    assert_eq!(call_id, "call_1");
                    assert!(output.contains("hello"));
                }
                other => panic!("unexpected item: {other:?}"),
            },
            other => panic!("unexpected event: {other:?}"),
        }

        drop(session);
    }
}
