//! High-level SDK facade over the Realtime protocol.
//!
//! The SDK exposes a simple async callback interface while keeping the low-level
//! protocol types accessible through `crate::protocol` when you need full control.

mod builder;
mod calls;
pub mod events;
mod handlers;
mod response;
mod session;
mod tools;
mod transport;

pub use builder::{Realtime, RealtimeBuilder};
pub use calls::Calls;
pub use events::{EventStream, SdkEvent};
pub use handlers::{EventHandlers, RawEventHandler, TextHandler, ToolCallHandler};
pub use response::ResponseBuilder;
pub use session::{Session, SessionHandle};
pub use tools::{BoxFuture as ToolFuture, ToolCall, ToolDefinition, ToolRegistry, ToolResult, ToolSpec};
