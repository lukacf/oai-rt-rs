use oai_rt_rs::sdk::{Realtime, ToolRegistry};
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

#[test]
fn builder_chain_compiles() {
    let _ = Realtime::builder()
        .api_key("k")
        .model("gpt-realtime")
        .voice("marin")
        .output_audio();
}

#[test]
fn tool_registry_collects_definitions() {
    let mut registry = ToolRegistry::new();
    registry.tool("echo", |args: EchoArgs| async move {
        Ok(EchoResp { echoed: args.text })
    });

    assert_eq!(registry.definitions().len(), 1);
    assert_eq!(registry.definitions()[0].name, "echo");
}
