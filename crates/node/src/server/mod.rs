//! Standalone HTTP server for a distributed Media Node (`aether-node`).
//!
//! Responsibilities:
//! - Persistent Node identity (`node_identity.json`) and operational asset/image lookup table
//!   (`node_inventory.json`) in `MEDIA_NODE_DATA_DIR`, strictly outside read-only media roots.
//! - Mutual HMAC-SHA256 authenticated internal endpoints (`/internal/v1/...`) with replay window.
//! - Asynchronous bounded batch scanning (`LocalNode::scan_library_streaming`) pushing signed
//!   `RemoteBatchSubmission`, `RemoteScanCompletion`, and `RemoteScanFailure` to Core.
//! - Direct browser → Node streaming (`/stream/v1/assets/{id}/content` and `/stream/v1/assets/{id}/image`)
//!   verified via short-lived Core-signed `PlaybackToken`, plus Core-relayed streaming
//!   (`/internal/v1/node/assets/{id}/content` and `/internal/v1/node/assets/{id}/image`).

mod control_transport;
mod outbound;
mod playback;
mod routes;
mod scans;
mod state;

pub use control_transport::{
    post_signed_to_core, post_signed_to_core_raw, send_signed_http_request,
};
pub use outbound::start_channel_worker;
pub use playback::{ActiveStreamGuard, track_stream_body};
pub use routes::{app_with_state, enroll_with_core};
pub use state::{NodeIdentityState, NodeOperationalInventory, NodeServerConfig, NodeServerState};

#[cfg(test)]
mod stream_signature_tests;
