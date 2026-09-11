//! Public GPT-Live API, independent of Realtime and the private experimental adapter.
//!
//! Start a primary WebSocket with [`SessionConfig`], or negotiate WebRTC with
//! [`LiveClient`]. Live audio runs continuously: `response.create` continues a
//! delegated backend, not the voice model. Transcript intervals are observations,
//! not turn identifiers or playback acknowledgments.

mod models;
mod error;
mod events;
mod codec;
mod client;
mod ws;
mod audio;
mod rest;
mod fork;
mod sip;
pub use sip::*;
pub use fork::*;
mod responses;
pub use responses::*;
pub use rest::*;
pub use audio::*;
pub use ws::*;
pub use client::{ClientOptions, LiveClient};
pub use codec::*;
pub use error::{Error, HttpBodyIssue, Result};
pub use events::*;
pub use models::*;
