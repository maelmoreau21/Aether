//! Node configuration, durable identity/inventory snapshots, and shared runtime state.

use crate::LocalNode;
use media_contracts::ids::AssetId;
use media_contracts::ids::ImageId;
use media_contracts::ids::NodeId;
use media_contracts::inventory::ReadGrant;
use media_contracts::inventory::ReadTarget;
use media_contracts::protocol::LibraryInventorySync;
use media_contracts::protocol::NodeInventoryAssetEntry;
use media_contracts::protocol::NodeInventoryImageEntry;
use media_contracts::protocol::ReplayWindow;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::io;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::RwLock;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU32;

#[derive(Debug, Clone)]
pub struct NodeServerConfig {
    pub bind: SocketAddr,
    pub data_dir: PathBuf,
    pub allowed_roots: Vec<PathBuf>,
    pub bootstrap_token: Option<String>,
    pub configured_node_id: Option<NodeId>,
    pub configured_name: Option<String>,
    pub public_url: Option<String>,
    pub batch_size: usize,
}

impl Default for NodeServerConfig {
    fn default() -> Self {
        Self {
            bind: SocketAddr::from(([127, 0, 0, 1], 3101)),
            data_dir: PathBuf::from("./data/node"),
            allowed_roots: Vec::new(),
            bootstrap_token: None,
            configured_node_id: None,
            configured_name: None,
            public_url: None,
            batch_size: 32,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodeIdentityState {
    pub node_id: NodeId,
    pub name: String,
    pub public_url: Option<String>,
    pub core_internal_url: Option<String>,
    pub shared_secret: Option<String>,
    pub key_id: String,
    pub revoked: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NodeOperationalInventory {
    pub libraries: HashMap<String, LibraryInventorySync>,
    pub assets: HashMap<String, NodeInventoryAssetEntry>,
    pub images: HashMap<String, NodeInventoryImageEntry>,
}

impl NodeOperationalInventory {
    pub fn apply_library_sync(&mut self, sync: LibraryInventorySync) {
        let lib_key = sync.library_id.as_str().to_owned();
        self.assets
            .retain(|_, entry| entry.library_id != sync.library_id);
        self.images
            .retain(|_, entry| entry.library_id != sync.library_id);
        for asset in &sync.assets {
            self.assets
                .insert(asset.asset_id.as_str().to_owned(), asset.clone());
        }
        for image in &sync.images {
            self.images
                .insert(image.image_id.as_str().to_owned(), image.clone());
        }
        self.libraries.insert(lib_key, sync);
    }

    pub fn grant_for_asset(&self, node_id: &NodeId, asset_id: &str) -> Option<ReadGrant> {
        let entry = self.assets.get(asset_id)?;
        Some(ReadGrant {
            target: ReadTarget::Asset(AssetId::new(asset_id)),
            library_id: entry.library_id.clone(),
            node_id: node_id.clone(),
            root_snapshot: entry.root_snapshot.clone(),
            relative_path: entry.relative_path.clone(),
            mime: entry.mime.clone(),
        })
    }

    pub fn grant_for_image(&self, node_id: &NodeId, image_id: &str) -> Option<ReadGrant> {
        let entry = self.images.get(image_id)?;
        Some(ReadGrant {
            target: ReadTarget::Image {
                id: ImageId::new(image_id),
                expected_size: entry.size,
                expected_modified_ns: entry.modified_ns.clone(),
            },
            library_id: entry.library_id.clone(),
            node_id: node_id.clone(),
            root_snapshot: entry.root_snapshot.clone(),
            relative_path: entry.relative_path.clone(),
            mime: entry.mime.clone(),
        })
    }
}

#[derive(Clone)]
pub struct NodeServerState {
    pub config: NodeServerConfig,
    pub identity: Arc<RwLock<NodeIdentityState>>,
    pub(super) identity_changes: Arc<tokio::sync::Mutex<()>>,
    pub inventory: Arc<RwLock<NodeOperationalInventory>>,
    pub replay_window: Arc<ReplayWindow>,
    pub active_scans: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
    pub active_streams: Arc<AtomicU32>,
    pub resources: Arc<crate::resources::Sampler>,
    pub local_node: LocalNode,
    pub(super) preparations: Arc<crate::packaging::Preparations>,
    pub trickplay: Arc<crate::trickplay::TrickplayManager>,
}

impl NodeServerState {
    pub async fn initialize(config: NodeServerConfig) -> io::Result<Self> {
        tokio::fs::create_dir_all(&config.data_dir).await?;
        let canonical_data = tokio::fs::canonicalize(&config.data_dir).await?;
        let mut canonical_roots = Vec::with_capacity(config.allowed_roots.len());
        for root in &config.allowed_roots {
            let c_root = tokio::fs::canonicalize(root).await?;
            if canonical_data.starts_with(&c_root) || c_root.starts_with(&canonical_data) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "MEDIA_NODE_DATA_DIR must be outside read-only media roots",
                ));
            }
            canonical_roots.push(c_root);
        }

        let identity_path = canonical_data.join("node_identity.json");
        let identity = if let Ok(bytes) = tokio::fs::read(&identity_path).await {
            let mut loaded: NodeIdentityState = serde_json::from_slice(&bytes)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            if let Some(name) = &config.configured_name {
                loaded.name = name.clone();
            }
            if config.public_url.is_some() {
                loaded.public_url = config.public_url.clone();
            }
            loaded
        } else {
            let node_id = config
                .configured_node_id
                .clone()
                .unwrap_or_else(|| NodeId::new(format!("node-{}", uuid::Uuid::new_v4())));
            let name = config
                .configured_name
                .clone()
                .unwrap_or_else(|| node_id.as_str().to_owned());
            let initial = NodeIdentityState {
                node_id,
                name,
                public_url: config.public_url.clone(),
                core_internal_url: None,
                shared_secret: None,
                key_id: "unpaired".to_owned(),
                revoked: false,
            };
            let serialized = serde_json::to_vec_pretty(&initial)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            tokio::fs::write(&identity_path, serialized).await?;
            initial
        };

        let inventory_path = canonical_data.join("node_inventory.json");
        let inventory = if let Ok(bytes) = tokio::fs::read(&inventory_path).await {
            serde_json::from_slice(&bytes).unwrap_or_default()
        } else {
            NodeOperationalInventory::default()
        };

        let mut updated_config = config;
        updated_config.data_dir = canonical_data;
        updated_config.allowed_roots = canonical_roots;

        let active_streams = Arc::new(AtomicU32::new(0));
        let trickplay = Arc::new(
            crate::trickplay::TrickplayManager::from_env()
                .with_active_streams(active_streams.clone()),
        );

        Ok(Self {
            resources: Arc::new(crate::resources::Sampler::new(
                updated_config.data_dir.clone(),
            )),
            config: updated_config,
            identity: Arc::new(RwLock::new(identity)),
            identity_changes: Arc::new(tokio::sync::Mutex::new(())),
            inventory: Arc::new(RwLock::new(inventory)),
            replay_window: Arc::new(ReplayWindow::new()),
            active_scans: Arc::new(Mutex::new(HashMap::new())),
            active_streams,
            local_node: LocalNode,
            preparations: Arc::new(crate::packaging::Preparations::default()),
            trickplay,
        })
    }

    pub async fn persist_identity(&self) -> io::Result<()> {
        let snapshot = self
            .identity
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let bytes = serde_json::to_vec_pretty(&snapshot)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let path = self.config.data_dir.join("node_identity.json");
        let temporary = self.config.data_dir.join("node_identity.pending");
        let mut options = tokio::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        options.mode(0o600);
        let mut file = options.open(&temporary).await?;
        use tokio::io::AsyncWriteExt;
        file.write_all(&bytes).await?;
        file.sync_all().await?;
        drop(file);
        tokio::fs::rename(temporary, path).await?;
        #[cfg(unix)]
        {
            let directory = self.config.data_dir.clone();
            tokio::task::spawn_blocking(move || std::fs::File::open(directory)?.sync_all())
                .await
                .map_err(io::Error::other)??;
        }
        Ok(())
    }

    pub async fn persist_inventory(&self) -> io::Result<()> {
        let snapshot = self
            .inventory
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let bytes = serde_json::to_vec(&snapshot)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let path = self.config.data_dir.join("node_inventory.json");
        tokio::fs::write(path, bytes).await
    }

    pub fn node_id(&self) -> NodeId {
        self.identity
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .node_id
            .clone()
    }
}
