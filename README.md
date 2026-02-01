# oai-rt-rs

A Rust client for the [OpenAI Realtime API](https://platform.openai.com/docs/guides/realtime).

## Features

- GA-aligned Realtime API protocol models (WebSocket + REST).
- Strongly typed `ClientEvent` and `ServerEvent` enums.
- WebRTC SDP signaling, SIP control endpoints, and call hangup.
- Async interface using `tokio` and `tokio-tungstenite`.
- Client-side validation for GA constraints (PCM 24kHz, output modalities, 15MB audio chunks).

## Quickstart (SDK)

```rust
use oai_rt_rs::{Realtime, ToolRegistry};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, JsonSchema)]
struct EchoArgs {
    text: String,
}

#[derive(Debug, Serialize)]
struct EchoResp {
    echoed: String,
}

#[tokio::main]
async fn main() -> oai_rt_rs::Result<()> {
    let mut tools = ToolRegistry::new();
    tools.tool_with_description("echo", "Echo back the input.", |args: EchoArgs| async move {
        Ok(EchoResp { echoed: args.text })
    });

    let mut session = Realtime::builder()
        .api_key("your-api-key")
        .model("gpt-realtime")
        .instructions("Be helpful.")
        .output_audio()
        .tools(tools)
        .on_text(|text| async move {
            println!("assistant: {text}");
            Ok(())
        })
        .connect_ws()
        .await?;

    let reply = session.ask("Hello!").await?;
    println!("received: {:?}", reply);
    Ok(())
}
```

## Response builder (high-level)

```rust
use oai_rt_rs::ResponseBuilder;

# async fn demo(session: &oai_rt_rs::RealtimeSession) -> oai_rt_rs::Result<()> {
ResponseBuilder::new()
    .output_text()
    .instructions("Be concise.")
    .input_text("Summarize this.")
    .send(session)
    .await?;
# Ok(())
# }
```

## Typed tools (macro)

```rust
use oai_rt_rs::{realtime_tool, ToolRegistry};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SumArgs {
    pub a: i32,
    pub b: i32,
}

#[derive(Debug, Serialize)]
pub struct SumResp {
    pub sum: i32,
}

realtime_tool!(SumTool: SumArgs => SumResp {
    name: "sum",
    description: "Add two integers.",
    |args: SumArgs| async move {
        Ok(SumResp { sum: args.a + args.b })
    }
});

# fn demo() {
let mut registry = ToolRegistry::new();
registry.register(SumTool);
# }
```

## Low-level protocol (full control)

```rust
use oai_rt_rs::RealtimeClient;
use oai_rt_rs::protocol::client_events::ClientEvent;
use oai_rt_rs::protocol::models::{SessionUpdate, SessionUpdateConfig, OutputModalities};

#[tokio::main]
async fn main() -> oai_rt_rs::Result<()> {
    let mut client = RealtimeClient::connect("your-api-key", None, None).await?;

    let session = SessionUpdate {
        config: SessionUpdateConfig {
            output_modalities: Some(OutputModalities::Audio),
            instructions: Some("You are a helpful assistant.".to_string()),
            ..SessionUpdateConfig::default()
        },
    };
    client
        .send(ClientEvent::SessionUpdate { event_id: None, session: Box::new(session) })
        .await?;

    while let Some(event) = client.next_event().await? {
        println!("Received event: {:?}", event);
    }
    Ok(())
}
```

## REST helpers (WebRTC/SIP)

```rust
use oai_rt_rs::{Calls};
use oai_rt_rs::protocol::models::{SessionConfig, SessionKind, OutputModalities};

# async fn demo() -> oai_rt_rs::Result<()> {
let rest = Calls::new("your-api-key")?;
let session = SessionConfig::new(
    SessionKind::Realtime,
    "gpt-realtime",
    OutputModalities::Audio,
);

// WebRTC (raw SDP) + call_id capture
let resp = rest.webrtc_offer_raw_with_call_id("v=0...".to_string()).await?;
println!("call_id: {:?}", resp.call_id);

// Hang up
if let Some(call_id) = resp.call_id.as_deref() {
    rest.hangup(call_id).await?;
}
# Ok(())
# }
```

## GA constraints (no beta)

- `output_modalities` must be exactly one of `audio` or `text`.
- `audio/pcm` rate is fixed at 24 kHz.
- `input_audio_buffer.append` chunks must be ≤ 15 MB (base64-decoded).
- Invalid GA inputs are rejected client-side with `Error::InvalidClientEvent`.

## MCP example

```rust
use oai_rt_rs::protocol::models::{
    McpToolConfig, RequireApproval, ApprovalMode, Tool, ToolChoice, ToolChoiceMode, SessionUpdate, SessionUpdateConfig,
};
use oai_rt_rs::protocol::client_events::ClientEvent;
use oai_rt_rs::protocol::models::SessionUpdate;

# async fn demo(mut client: oai_rt_rs::RealtimeClient) -> oai_rt_rs::Result<()> {
let mcp = Tool::Mcp(McpToolConfig {
    server_label: "weather".to_string(),
    server_url: Some("https://mcp.example.com".to_string()),
    require_approval: Some(RequireApproval::Mode(ApprovalMode::Always)),
    ..McpToolConfig::default()
});

let session = SessionUpdate {
    config: SessionUpdateConfig {
        tools: Some(vec![mcp]),
        tool_choice: Some(ToolChoice::Mode(ToolChoiceMode::Auto)),
        ..SessionUpdateConfig::default()
    },
};

client.send(ClientEvent::SessionUpdate { event_id: None, session: Box::new(session) }).await?;
# Ok(())
# }
```

## MCP approval flow (items)

```rust
use oai_rt_rs::protocol::models::{Item, ItemStatus};
use oai_rt_rs::protocol::client_events::ClientEvent;

# async fn demo(mut client: oai_rt_rs::RealtimeClient) -> oai_rt_rs::Result<()> {
let request = Item::McpApprovalRequest {
    id: Some("item_req_1".to_string()),
    status: Some(ItemStatus::InProgress),
    server_label: "weather".to_string(),
    name: "get_forecast".to_string(),
    arguments: r#"{"city":"Paris"}"#.to_string(),
};

client.send(ClientEvent::ConversationItemCreate {
    event_id: None,
    previous_item_id: None,
    item: Box::new(request),
}).await?;

let response = Item::McpApprovalResponse {
    id: Some("item_resp_1".to_string()),
    status: Some(ItemStatus::Completed),
    approval_request_id: "item_req_1".to_string(),
    approve: true,
    reason: None,
};

client.send(ClientEvent::ConversationItemCreate {
    event_id: None,
    previous_item_id: None,
    item: Box::new(response),
}).await?;
# Ok(())
# }
```

## Response creation

```rust
use oai_rt_rs::protocol::client_events::ClientEvent;
use oai_rt_rs::protocol::models::{ResponseConfig, InputItem, ContentPart, OutputModalities};

# async fn demo(mut client: oai_rt_rs::RealtimeClient) -> oai_rt_rs::Result<()> {
let response = ResponseConfig {
    output_modalities: Some(OutputModalities::Text),
    input: Some(vec![InputItem::Message {
        id: None,
        role: oai_rt_rs::protocol::models::Role::User,
        content: vec![ContentPart::InputText { text: "Hello".to_string() }],
    }]),
    ..ResponseConfig::default()
};

client.send(ClientEvent::ResponseCreate {
    event_id: None,
    response: Some(Box::new(response)),
}).await?;
# Ok(())
# }
```
