//! High-level SDK facade over the Realtime protocol.
//!
//! The SDK exposes a simple async callback interface while keeping the low-level
//! protocol types accessible through `crate::protocol` when you need full control.

mod builder;
mod handlers;
mod session;
mod tools;
mod transport;

pub use builder::{Realtime, RealtimeBuilder};
pub use handlers::{EventHandlers, RawEventHandler, TextHandler, ToolCallHandler};
pub use session::{Session, SessionHandle};
pub use tools::{ToolCall, ToolDefinition, ToolRegistry, ToolResult};
