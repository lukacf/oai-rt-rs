use crate::Result;
use schemars::schema::RootSchema;
use std::sync::Arc;
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

type ToolHandler = Box<dyn Fn(Value) -> BoxFuture<Result<Value>> + Send + Sync>;

#[derive(Clone, Debug)]
pub struct ToolDefinition {
    pub name: String,
    pub description: Option<String>,
    pub schema: RootSchema,
}

#[derive(Clone, Debug)]
pub struct ToolCall {
    pub name: String,
    pub call_id: String,
    pub arguments: Value,
}

#[derive(Clone, Debug)]
pub struct ToolResult {
    pub call_id: String,
    pub output: Value,
}

#[derive(Default)]
pub struct ToolRegistry {
    defs: Vec<ToolDefinition>,
    handlers: HashMap<String, ToolHandler>,
}

impl ToolRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn definitions(&self) -> &[ToolDefinition] {
        &self.defs
    }

    pub fn tool<TArgs, TResp, F, Fut>(&mut self, name: &str, handler: F)
    where
        TArgs: DeserializeOwned + JsonSchema + Send + 'static,
        TResp: Serialize + Send + 'static,
        F: Fn(TArgs) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<TResp>> + Send + 'static,
    {
        let schema = schemars::schema_for!(TArgs);
        let name = name.to_string();
        let entry = ToolDefinition {
            name: name.clone(),
            description: None,
            schema,
        };
        self.defs.push(entry);

        let user_handler = Arc::new(handler);
        let handler = move |value: Value| -> BoxFuture<Result<Value>> {
            let user_handler = Arc::clone(&user_handler);
            Box::pin(async move {
                let args: TArgs = serde_json::from_value(value)
                    .map_err(|e| crate::Error::InvalidClientEvent(e.to_string()))?;
                let resp = user_handler(args).await?;
                serde_json::to_value(resp)
                    .map_err(|e| crate::Error::InvalidClientEvent(e.to_string()))
            })
        };

        self.handlers.insert(name, Box::new(handler));
    }

    /// Dispatch a tool call to the registered handler.
    ///
    /// # Errors
    /// Returns an error if the tool is unknown or execution fails.
    pub async fn dispatch(&self, call: ToolCall) -> Result<ToolResult> {
        let handler = self.handlers.get(&call.name).ok_or_else(|| {
            crate::Error::InvalidClientEvent(format!("unknown tool: {}", call.name))
        })?;
        let output = handler(call.arguments).await?;
        Ok(ToolResult { call_id: call.call_id, output })
    }
}
