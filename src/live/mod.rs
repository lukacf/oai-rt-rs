//! Public GPT-Live API, independent of Realtime and the private experimental adapter.
//!
//! Start a primary WebSocket with [`SessionConfig`], or negotiate WebRTC with
//! [`LiveClient`]. Live audio runs continuously: `response.create` continues a
//! delegated backend, not the voice model. Transcript intervals are observations,
//! not turn identifiers or playback acknowledgments.

#![doc = include_str!("../../docs/live.md")]

mod audio;
mod client;
mod codec;
mod error;
mod events;
mod fork;
mod models;
mod responses;
mod rest;
mod sip;
mod ws;
pub use audio::*;
pub use client::{ClientOptions, LiveClient};
pub use codec::*;
pub use error::{Error, HttpBodyIssue, Result};
pub use events::*;
pub use fork::*;
pub use models::*;
pub use responses::*;
pub use rest::*;
pub use sip::*;
pub use ws::*;
