//! Installed signed artifacts under data/plugins. Database retains durable
//! authorization and backup snapshots; the worker consumes verified on-disk Wasm.
use crate::{database::query_as, identity::IdentityStore};
use base64::{Engine, engine::general_purpose::STANDARD};
use media_extension_sdk::{MAX_MODULE, Package};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io,
    path::{Path, PathBuf},
};
use tokio::io::AsyncReadExt;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginStorageHealth {
    pub path: String,
    pub exists: bool,
    pub writable: bool,
    pub healthy: bool,
    pub installed_count: usize,
    pub on_disk_count: usize,
    pub missing_count: usize,
    pub corrupted_count: usize,
    pub error: Option<String>,
}

fn directory(store: &IdentityStore, id: &str) -> io::Result<PathBuf> {
    if id.is_empty()
        || id.len() > 80
        || id == "."
        || id == ".."
        || id.ends_with('.')
        || !id
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"._-".contains(&c))
    {
        return Err(io::Error::other("invalid plugin directory identity"));
    }
    Ok(store.plugins_path().join(id))
}

fn real_directory(metadata: &std::fs::Metadata) -> bool {
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return false;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return false;
        }
    }
    true
}

async fn clean_staging(root: &Path) {
    if let Ok(mut entries) = tokio::fs::read_dir(root).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            if let Some(name) = entry.file_name().to_str()
                && name.starts_with(".install-")
            {
                let _ = clean_known(&entry.path()).await;
                let _ = tokio::fs::remove_dir_all(entry.path()).await;
            }
        }
    }
}

async fn root(store: &IdentityStore) -> io::Result<()> {
    tokio::fs::create_dir_all(store.plugins_path()).await?;
    let metadata = tokio::fs::symlink_metadata(store.plugins_path()).await?;
    if !real_directory(&metadata) {
        return Err(io::Error::other("plugin root must be a real directory"));
    }
    clean_staging(store.plugins_path()).await;
    Ok(())
}

async fn read(root: &Path, path: &Path, maximum: usize) -> io::Result<Vec<u8>> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| io::Error::other("plugin path outside storage root"))?;
    let opened = media_node::OpenedFile::open_in_root(root, relative).await?;
    if opened.size() > maximum as u64 {
        return Err(io::Error::other("plugin artifact too large"));
    }
    let mut bytes = Vec::new();
    tokio::fs::File::from_std(opened.try_clone_std()?)
        .take(maximum as u64 + 1)
        .read_to_end(&mut bytes)
        .await?;
    if bytes.len() > maximum {
        return Err(io::Error::other("plugin artifact too large"));
    }
    opened.verify_unchanged_sync()?;
    Ok(bytes)
}

async fn clean_known(path: &Path) -> io::Result<()> {
    // No recursive deletion and no following links: only our three named artifacts.
    let metadata = match tokio::fs::symlink_metadata(path).await {
        Ok(value) => value,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if !real_directory(&metadata) {
        return Err(io::Error::other("invalid plugin directory"));
    }
    for name in ["package.json", "manifest.json", "module.wasm"] {
        match tokio::fs::remove_file(path.join(name)).await {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    tokio::fs::remove_dir(path).await
}

pub async fn persist(store: &IdentityStore, package: &Package) -> io::Result<()> {
    persist_internal(store, package, false).await
}

pub async fn persist_upgrade(store: &IdentityStore, package: &Package) -> io::Result<()> {
    persist_internal(store, package, true).await
}

async fn persist_internal(
    store: &IdentityStore,
    package: &Package,
    allow_upgrade: bool,
) -> io::Result<()> {
    root(store).await?;
    let destination = directory(store, &package.manifest.id)?;
    let bytes = serde_json::to_vec(package).map_err(io::Error::other)?;
    let module = STANDARD.decode(&package.wasm).map_err(io::Error::other)?;
    if module.len() > MAX_MODULE || format!("{:x}", Sha256::digest(&module)) != package.sha256 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "plugin artifact integrity failure",
        ));
    }

    if tokio::fs::try_exists(&destination).await? && !allow_upgrade {
        let package_file = destination.join("package.json");
        let manifest_file = destination.join("manifest.json");
        let module_file = destination.join("module.wasm");

        let stored_package = match read(store.plugins_path(), &package_file, 400000).await {
            Ok(stored_bytes) => serde_json::from_slice::<Package>(&stored_bytes).ok(),
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) if e.kind() == io::ErrorKind::InvalidData => None,
            Err(e) => return Err(e),
        };

        if let Some(stored) = stored_package {
            let stored_bytes = serde_json::to_vec(&stored).map_err(io::Error::other)?;
            if stored.manifest.id != package.manifest.id
                || stored.manifest.version != package.manifest.version
                || stored_bytes != bytes
            {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "plugin artifact conflicts with installed snapshot",
                ));
            }

            let manifest_ok = tokio::fs::try_exists(&manifest_file).await.unwrap_or(false);
            let module_ok = match read(store.plugins_path(), &module_file, MAX_MODULE).await {
                Ok(bytes) => format!("{:x}", Sha256::digest(&bytes)) == package.sha256,
                Err(_) => false,
            };

            if manifest_ok && module_ok {
                return Ok(());
            }
        }
    }

    let staging = store
        .plugins_path()
        .join(format!(".install-{}", uuid::Uuid::new_v4()));
    tokio::fs::create_dir(&staging).await?;
    let result = async {
        tokio::fs::write(staging.join("package.json"), &bytes).await?;
        tokio::fs::write(
            staging.join("manifest.json"),
            serde_json::to_vec_pretty(&package.manifest).map_err(io::Error::other)?,
        )
        .await?;
        tokio::fs::write(staging.join("module.wasm"), &module).await?;
        if tokio::fs::try_exists(&destination).await? {
            tokio::fs::write(destination.join("package.json"), bytes).await?;
            tokio::fs::write(
                destination.join("manifest.json"),
                serde_json::to_vec_pretty(&package.manifest).map_err(io::Error::other)?,
            )
            .await?;
            tokio::fs::write(destination.join("module.wasm"), module).await?;
            clean_known(&staging).await?;
            Ok(())
        } else {
            tokio::fs::rename(&staging, &destination).await
        }
    }
    .await;
    if result.is_err() {
        let _ = clean_known(&staging).await;
        let _ = tokio::fs::remove_dir_all(&staging).await;
    }
    result
}

pub async fn wasm(_store: &IdentityStore, package: &Package) -> io::Result<String> {
    // Callers have loaded this package from the durable extensions row. Execute
    // its verified bytes directly: local files are a disposable cache and may
    // lag a package upgrade performed by another Core.
    let module = STANDARD.decode(&package.wasm).map_err(io::Error::other)?;
    if module.len() > MAX_MODULE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "plugin module too large",
        ));
    }
    if format!("{:x}", Sha256::digest(&module)) != package.sha256 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "plugin module integrity failure",
        ));
    }
    Ok(package.wasm.clone())
}

pub async fn remove(store: &IdentityStore, id: &str) -> io::Result<()> {
    let directory = directory(store, id)?;
    if !tokio::fs::try_exists(store.plugins_path()).await? {
        return Ok(());
    }
    // Only validate that plugin root is a real directory if it exists
    if let Ok(metadata) = tokio::fs::symlink_metadata(store.plugins_path()).await
        && !real_directory(&metadata)
    {
        return Err(io::Error::other("plugin root must be a real directory"));
    }
    clean_known(&directory).await
}

pub async fn sync_installed(store: &IdentityStore) -> io::Result<usize> {
    root(store).await?;
    clean_staging(store.plugins_path()).await;
    let rows: Vec<(String, String)> =
        query_as("SELECT id, package_json FROM extensions ORDER BY id")
            .fetch_all(store.pool())
            .await
            .map_err(io::Error::other)?;
    let total = rows.len();
    let mut synced = 0;
    let mut failed = Vec::new();
    for (id, bytes) in rows {
        let package: Package = match serde_json::from_str(&bytes) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("Plugin '{id}' stored package JSON invalid: {e}");
                failed.push(id);
                continue;
            }
        };
        let synced_package = match persist(store, &package).await {
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                persist_upgrade(store, &package).await
            }
            result => result,
        };
        match synced_package {
            Ok(()) => synced += 1,
            Err(e) => {
                eprintln!(
                    "Plugin '{id}' synchronization failed ({:?}): {}",
                    e.kind(),
                    e
                );
                failed.push(id);
            }
        }
    }
    if !failed.is_empty() && synced == 0 && total > 0 {
        return Err(io::Error::other(format!(
            "Failed to synchronize all {total} installed plugin(s): {failed:?}"
        )));
    }
    Ok(synced)
}

pub async fn check_health(store: &IdentityStore) -> PluginStorageHealth {
    let path = store.plugins_path().to_string_lossy().to_string();
    let exists = match tokio::fs::symlink_metadata(store.plugins_path()).await {
        Ok(m) => real_directory(&m),
        Err(_) => false,
    };
    if !exists && let Err(e) = root(store).await {
        return PluginStorageHealth {
            path,
            exists: false,
            writable: false,
            healthy: false,
            installed_count: 0,
            on_disk_count: 0,
            missing_count: 0,
            corrupted_count: 0,
            error: Some(format!("Plugin root unavailable ({:?}): {e}", e.kind())),
        };
    }

    let probe = store
        .plugins_path()
        .join(format!(".probe-health-{}", uuid::Uuid::new_v4()));
    let writable = match tokio::fs::write(&probe, b"probe").await {
        Ok(()) => {
            let _ = tokio::fs::remove_file(&probe).await;
            true
        }
        Err(e) => {
            return PluginStorageHealth {
                path,
                exists: true,
                writable: false,
                healthy: false,
                installed_count: 0,
                on_disk_count: 0,
                missing_count: 0,
                corrupted_count: 0,
                error: Some(format!("Plugin storage not writable ({:?}): {e}", e.kind())),
            };
        }
    };

    let rows: Result<Vec<(String, String)>, _> =
        query_as("SELECT id, package_json FROM extensions ORDER BY id")
            .fetch_all(store.pool())
            .await;

    let rows = match rows {
        Ok(r) => r,
        Err(e) => {
            return PluginStorageHealth {
                path,
                exists: true,
                writable,
                healthy: false,
                installed_count: 0,
                on_disk_count: 0,
                missing_count: 0,
                corrupted_count: 0,
                error: Some(format!("Database query failed: {e}")),
            };
        }
    };

    let installed_count = rows.len();
    let mut on_disk_count = 0;
    let mut missing_count = 0;
    let mut corrupted_count = 0;
    let mut errors = Vec::new();

    for (id, package_json) in rows {
        let package: Package = match serde_json::from_str(&package_json) {
            Ok(p) => p,
            Err(e) => {
                errors.push(format!("Extension '{id}' has invalid package JSON: {e}"));
                corrupted_count += 1;
                continue;
            }
        };
        let dest = match directory(store, &id) {
            Ok(d) => d,
            Err(e) => {
                errors.push(format!("Extension '{id}' invalid directory: {e}"));
                corrupted_count += 1;
                continue;
            }
        };
        if !tokio::fs::try_exists(&dest).await.unwrap_or(false) {
            missing_count += 1;
            continue;
        }
        let pkg_ok = read(store.plugins_path(), &dest.join("package.json"), 400000)
            .await
            .is_ok();
        let manifest_ok = tokio::fs::try_exists(&dest.join("manifest.json"))
            .await
            .unwrap_or(false);
        let module_bytes = read(store.plugins_path(), &dest.join("module.wasm"), MAX_MODULE).await;
        let module_ok = match &module_bytes {
            Ok(b) => format!("{:x}", Sha256::digest(b)) == package.sha256,
            Err(_) => false,
        };
        if pkg_ok && manifest_ok && module_ok {
            on_disk_count += 1;
        } else if !pkg_ok && !manifest_ok && module_bytes.is_err() {
            missing_count += 1;
        } else {
            corrupted_count += 1;
            errors.push(format!(
                "Extension '{id}' has missing or corrupted files on disk"
            ));
        }
    }

    let healthy = writable && missing_count == 0 && corrupted_count == 0 && errors.is_empty();
    let error = if errors.is_empty() {
        None
    } else {
        Some(errors.join("; "))
    };

    PluginStorageHealth {
        path,
        exists: true,
        writable,
        healthy,
        installed_count,
        on_disk_count,
        missing_count,
        corrupted_count,
        error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn initialization_keeps_empty_installation_free_of_phantom_extensions() {
        let root = tempfile::tempdir().unwrap();
        let store = IdentityStore::open(&root.path().join("core.sqlite3"))
            .await
            .unwrap();
        store
            .bootstrap(
                "test-admin",
                zeroize::Zeroizing::new("disposable fixture password".into()),
            )
            .await
            .unwrap();
        let auth = crate::auth::AuthState::new(store.clone(), "http://localhost".into())
            .await
            .unwrap();
        crate::extensions::initialize(&auth).await.unwrap();
        crate::extensions::initialize(&auth).await.unwrap();
        let count: i64 = crate::database::query_scalar("SELECT COUNT(*) FROM extensions")
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert_eq!(count, 0);
        assert_eq!(sync_installed(&store).await.unwrap(), 0);
        assert!(check_health(&store).await.healthy);
    }

    #[tokio::test]
    async fn installed_files_are_bounded_verified_and_removed() {
        let root = tempfile::tempdir().unwrap();
        let store = IdentityStore::open(&root.path().join("core.sqlite3"))
            .await
            .unwrap();
        assert_eq!(store.plugins_path(), root.path().join("plugins"));
        let module = b"legal fixture bytes";
        let package = Package {
            manifest: media_extension_sdk::Manifest {
                id: "independent.plugin".into(),
                ..Default::default()
            },
            publisher_id: "anyone".into(),
            wasm: STANDARD.encode(module),
            sha256: format!("{:x}", Sha256::digest(module)),
            signature: String::new(),
        };
        persist(&store, &package).await.unwrap();
        assert_eq!(wasm(&store, &package).await.unwrap(), package.wasm);
        // Existing snapshots may have been pretty-printed by backup/recovery tools.
        tokio::fs::write(
            store.plugins_path().join("independent.plugin/package.json"),
            serde_json::to_vec_pretty(&package).unwrap(),
        )
        .await
        .unwrap();
        persist(&store, &package).await.unwrap();
        let mut different = package.clone();
        different.manifest.version = "2.0.0".into();
        assert_eq!(
            persist(&store, &different).await.unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        let directory = store.plugins_path().join("independent.plugin");
        assert!(directory.join("manifest.json").exists());
        assert!(directory.join("package.json").exists());
        tokio::fs::write(directory.join("module.wasm"), b"tampered")
            .await
            .unwrap();
        // The package snapshot remains authoritative even when the local cache
        // is corrupt; startup reconciliation will rebuild the cache.
        assert_eq!(wasm(&store, &package).await.unwrap(), package.wasm);
        remove(&store, &package.manifest.id).await.unwrap();
        assert!(!directory.exists());
        crate::database::query(
            "INSERT INTO extension_publishers(id,public_key,active) VALUES('anyone','fixture',1)",
        )
        .execute(store.pool())
        .await
        .unwrap();
        crate::database::query("INSERT INTO extensions(id,publisher_id,package_json,generation) VALUES(?,?,?,'fixture')").bind(&package.manifest.id).bind("anyone").bind(serde_json::to_string(&package).unwrap()).execute(store.pool()).await.unwrap();
        assert_eq!(sync_installed(&store).await.unwrap(), 1);
        assert!(directory.join("module.wasm").exists());
        let mut upgraded = package.clone();
        upgraded.manifest.version = "2.0.0".into();
        let upgraded_bytes = serde_json::to_string(&upgraded).unwrap();
        crate::database::query("UPDATE extensions SET package_json=? WHERE id=?")
            .bind(upgraded_bytes)
            .bind(&package.manifest.id)
            .execute(store.pool())
            .await
            .unwrap();
        assert_eq!(sync_installed(&store).await.unwrap(), 1);
        let cached = read(
            store.plugins_path(),
            &directory.join("package.json"),
            400000,
        )
        .await
        .unwrap();
        assert_eq!(
            serde_json::from_slice::<Package>(&cached)
                .unwrap()
                .manifest
                .version,
            "2.0.0"
        );
        let reopened = IdentityStore::open(&root.path().join("core.sqlite3"))
            .await
            .unwrap();
        assert_eq!(wasm(&reopened, &package).await.unwrap(), package.wasm);
        // Uninstalling when the directory does not exist must succeed cleanly
        remove(&store, "nonexistent.plugin").await.unwrap();
        for id in ["..", ".", "../outside", "a/b", "a\\b", "end."] {
            assert!(directory_for_test(&store, id).is_err());
        }
    }

    fn directory_for_test(store: &IdentityStore, id: &str) -> io::Result<PathBuf> {
        directory(store, id)
    }
}
