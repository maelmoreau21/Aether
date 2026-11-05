#![allow(clippy::type_complexity, clippy::collapsible_if)]

//! Synchronized Community Watch Parties (SyncPlay) for Aether.
//! Media playback coordination runs on Core via WebSockets while video/audio streaming remains strictly on Media Nodes.
//!
//! Module layout:
//! - `error`: HTTP error type shared by the REST handlers.
//! - `util`: limits and epoch helpers.
//! - `types`: serializable payloads and WebSocket message contracts.
//! - `hub`: in-memory rooms and the `WatchPartyHub` registry.
//! - `access`: library / parental authorization helper.
//! - `http`: REST router and handlers.
//! - `ws`: real-time WebSocket handler and client event processing.
//! - `cleanup`: expired parties and invites maintenance.

mod access;
mod cleanup;
mod error;
mod http;
mod hub;
mod types;
mod util;
mod ws;

#[cfg(test)]
mod tests;

pub use access::can_user_access_media;
pub use cleanup::clean_expired_parties;
pub use error::PartyApiError;
pub use http::router;
pub use hub::{LiveParticipant, PartyRoom, WatchPartyHub};
pub use types::*;
