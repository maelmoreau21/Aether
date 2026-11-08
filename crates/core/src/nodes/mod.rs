//! Distributed Media Nodes: registry, identity lifecycle, signed control, scans and playback.
//! The facade preserves the existing `crate::nodes` public paths.

mod channel;
mod management;
mod playback;
mod registry;
mod scans;
mod transport;

use crate::auth::AuthState;
use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::routing::get;
use axum::routing::post;
pub use channel::{
    ACTIVE_CHANNELS, CommandRefused, NodeChannel, PENDING_STREAMS, RelayStreamResponse,
    channel_is_active, live_channel,
};
use channel::{node_channel, receive_node_heartbeat, receive_stream_uplink};
pub use management::{CreateEnrollmentTokenInput, PairNodeAdminInput};
use management::{
    delete_enrollment_token_admin, enroll_node, generate_enrollment_token_admin,
    list_enrollment_tokens_admin, list_nodes, pair_node_admin, reconnect_node, revoke_node_admin,
    rotate_node_secret,
};
use media_contracts::protocol::ReplayWindow;
use playback::authorize_playback;
pub use playback::{build_direct_playback_url, relay_from_remote_node};
pub use registry::{
    AdminNodeView, MediaNodeRecord, NodeLibraryRef, NodeRuntimeEntry, NodeRuntimeRegistry,
    NodeRuntimeTelemetry, NodeScanSummary, effective_connection_mode, ensure_node_registry_warm,
    load_admin_nodes, load_node, load_node_entry, node_is_stale, node_stale_limit_secs,
    refresh_node_in_registry, score_node_entry,
};
pub use scans::{build_library_inventory_sync, dispatch_remote_scan, sync_all_libraries_for_node};
pub(crate) use scans::{
    cancel_remote_scan, dispatch_remote_task, scan_run_authorized, task_run_authorized,
};
use scans::{
    receive_music_track, receive_scan_batch, receive_scan_complete, receive_scan_fail,
    receive_task_complete,
};
use std::sync::Arc;

#[derive(Clone)]
pub struct NodesState {
    pub auth: AuthState,
    pub replay_window: Arc<ReplayWindow>,
}

pub fn router(auth: AuthState, replay_window: Arc<ReplayWindow>) -> Router {
    let state = NodesState {
        auth,
        replay_window,
    };
    Router::new()
        .route("/api/v1/admin/nodes", get(list_nodes))
        .route("/internal/v1/core/music/tracks", post(receive_music_track))
        .route(
            "/api/v1/admin/nodes/enrollment-tokens",
            post(generate_enrollment_token_admin).get(list_enrollment_tokens_admin),
        )
        .route(
            "/api/v1/admin/nodes/enrollment-tokens/{id}",
            axum::routing::delete(delete_enrollment_token_admin),
        )
        .route("/api/v1/nodes/enroll", post(enroll_node))
        .route("/api/v1/admin/nodes/pair", post(pair_node_admin))
        .route("/api/v1/admin/nodes/{id}/reconnect", post(reconnect_node))
        .route(
            "/api/v1/admin/nodes/{id}/rotate-secret",
            post(rotate_node_secret),
        )
        .route("/api/v1/admin/nodes/{id}/revoke", post(revoke_node_admin))
        .route("/internal/v1/core/heartbeat", post(receive_node_heartbeat))
        .route("/internal/v1/core/nodes/{id}/channel", post(node_channel))
        .route(
            "/internal/v1/core/nodes/{id}/stream-uplink/{stream_id}",
            post(receive_stream_uplink),
        )
        .route(
            "/internal/v1/core/playback/authorize",
            post(authorize_playback),
        )
        .route(
            "/internal/v1/core/tasks/{scan_id}/complete",
            post(receive_task_complete),
        )
        .route(
            "/internal/v1/core/scans/{scan_id}/batches",
            post(receive_scan_batch),
        )
        .route(
            "/internal/v1/core/scans/{scan_id}/complete",
            post(receive_scan_complete),
        )
        .route(
            "/internal/v1/core/scans/{scan_id}/fail",
            post(receive_scan_fail),
        )
        .layer(DefaultBodyLimit::max(8 * 1024 * 1024))
        .with_state(state)
}

#[cfg(test)]
#[path = "tests.rs"]
mod transport_policy_tests;
