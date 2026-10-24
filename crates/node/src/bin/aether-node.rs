use media_contracts::ids::NodeId;
use media_node::server::{NodeServerConfig, NodeServerState, app_with_state};
use std::{net::SocketAddr, path::PathBuf, time::Duration};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args
        .iter()
        .any(|a| a == "--version" || a == "-V" || a == "-v")
    {
        println!(
            "aether-node {}",
            media_contracts::version::current_version()
        );
        return Ok(());
    }

    let bind_str = match std::env::var("MEDIA_NODE_BIND") {
        Ok(b) if !b.trim().is_empty() => b,
        _ => {
            let addr =
                std::env::var("MEDIA_NODE_BIND_ADDR").unwrap_or_else(|_| "0.0.0.0".to_owned());
            let port = std::env::var("MEDIA_NODE_PORT").unwrap_or_else(|_| "3101".to_owned());
            format!("{addr}:{port}")
        }
    };
    let bind: SocketAddr = bind_str
        .parse()
        .map_err(|e| format!("invalid MEDIA_NODE_BIND ({bind_str}): {e}"))?;

    let data_dir = std::env::var("MEDIA_NODE_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/var/lib/aether-node"));

    let allowed_roots: Vec<PathBuf> = std::env::var("MEDIA_NODE_ROOTS")
        .unwrap_or_default()
        .split(|character| character == ';' || (!cfg!(windows) && character == ':'))
        .filter(|s| !s.trim().is_empty())
        .map(|s| PathBuf::from(s.trim()))
        .collect();

    if allowed_roots.is_empty() {
        return Err("MEDIA_NODE_ROOTS must contain at least one existing directory".into());
    }

    let bootstrap_token = std::env::var("MEDIA_NODE_BOOTSTRAP_TOKEN")
        .or_else(|_| std::env::var("MEDIA_NODE_PAIRING_TOKEN"))
        .ok()
        .filter(|s| !s.trim().is_empty());
    if bootstrap_token
        .as_ref()
        .is_some_and(|s| s.len() < 48 || !s.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Err("pairing secret must be at least 24 random bytes encoded as hex".into());
    }
    let configured_node_id = std::env::var("MEDIA_NODE_ID")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(NodeId::new);
    let configured_name = std::env::var("MEDIA_NODE_NAME")
        .ok()
        .filter(|s| !s.trim().is_empty());
    // Explicit empty value clears a previously persisted direct-stream origin.
    let public_url = std::env::var("MEDIA_NODE_PUBLIC_URL").ok();
    let batch_size = std::env::var("MEDIA_NODE_SCAN_BATCH_SIZE")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(32)
        .clamp(1, 128);

    let config = NodeServerConfig {
        bind,
        data_dir,
        allowed_roots,
        bootstrap_token,
        configured_node_id,
        configured_name,
        public_url,
        batch_size,
    };

    let state = NodeServerState::initialize(config).await?;
    let node_id = state.node_id();
    println!("aether-node ({node_id}) listening on http://{bind}");

    let core_url = std::env::var("MEDIA_CORE_URL")
        .or_else(|_| std::env::var("MEDIA_CORE_INTERNAL_URL"))
        .ok()
        .filter(|s| !s.trim().is_empty());
    let enrollment_token = std::env::var("MEDIA_NODE_ENROLLMENT_TOKEN")
        .ok()
        .filter(|s| !s.trim().is_empty());

    let is_paired = state
        .identity
        .read()
        .map(|id| id.shared_secret.is_some() && !id.revoked)
        .unwrap_or(false);

    if !is_paired {
        if let (Some(c_url), Some(token)) = (&core_url, &enrollment_token) {
            println!("Attempting auto-enrollment with Core at {c_url}...");
            match media_node::server::enroll_with_core(c_url, token, &state).await {
                Ok(_) => {
                    println!("Node successfully enrolled with Core! Durable identity saved.");
                }
                Err(err) => {
                    eprintln!("Failed to enroll with Core: {err}");
                }
            }
        }
    } else if let Some(c_url) = &core_url {
        let changed = {
            let mut id = state.identity.write().unwrap_or_else(|e| e.into_inner());
            if id.core_internal_url.as_deref() != Some(c_url.as_str()) {
                id.core_internal_url = Some(c_url.clone());
                true
            } else {
                false
            }
        };
        if changed {
            let _ = state.persist_identity().await;
        }
    }

    // Outbound persistent bi-directional control channel
    let channel_state = state.clone();
    media_node::server::start_channel_worker(channel_state);

    // Periodic heartbeat push to Core when paired (legacy/fallback)
    let hb_state = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(15));
        loop {
            interval.tick().await;
            let id = hb_state
                .identity
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            if id.revoked || id.shared_secret.is_none() || id.core_internal_url.is_none() {
                continue;
            }
            let active_scans = hb_state
                .active_scans
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .len() as u32;
            let indexed_assets = hb_state
                .inventory
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .assets
                .len();
            let roots = hb_state
                .config
                .allowed_roots
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect();
            let payload = media_contracts::protocol::HeartbeatPayload {
                protocol_version: media_contracts::protocol::PROTOCOL_VERSION.to_owned(),
                version: Some(media_contracts::version::current_version().to_string()),
                node_id: id.node_id,
                roots,
                public_url: id.public_url,
                active_scans,
                active_streams: hb_state
                    .active_streams
                    .load(std::sync::atomic::Ordering::Relaxed),
                indexed_assets,
                timestamp: media_contracts::protocol::unix_now(),
            };
            let _ = media_node::server::post_signed_to_core(
                &hb_state,
                "/internal/v1/core/heartbeat",
                &payload,
            )
            .await;
        }
    });

    let app = app_with_state(state);
    let listener = TcpListener::bind(bind).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}
