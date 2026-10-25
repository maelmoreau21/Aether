//! Installed signed artifacts under data/plugins. Database retains durable
//! authorization and backup snapshots; the worker consumes verified on-disk Wasm.
use crate::{database::query_scalar, identity::IdentityStore};
use base64::{Engine, engine::general_purpose::STANDARD};
use media_extension_sdk::{MAX_MODULE, Package};
use sha2::{Digest, Sha256};
use std::{
    io,
    path::{Path, PathBuf},
};
use tokio::io::AsyncReadExt;
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
async fn root(store: &IdentityStore) -> io::Result<()> {
    tokio::fs::create_dir_all(store.plugins_path()).await?;
    let metadata = tokio::fs::symlink_metadata(store.plugins_path()).await?;
    if !real_directory(&metadata) {
        return Err(io::Error::other("plugin root must be a real directory"));
    }
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
    root(store).await?;
    let destination = directory(store, &package.manifest.id)?;
    let bytes = serde_json::to_vec(package).map_err(io::Error::other)?;
    if tokio::fs::try_exists(&destination).await? {
        let stored = read(
            store.plugins_path(),
            &destination.join("package.json"),
            400000,
        )
        .await?;
        if stored != bytes {
            return Err(io::Error::other(
                "plugin artifact conflicts with installed snapshot",
            ));
        }
        let module = read(
            store.plugins_path(),
            &destination.join("module.wasm"),
            MAX_MODULE,
        )
        .await?;
        if format!("{:x}", Sha256::digest(module)) != package.sha256 {
            return Err(io::Error::other("plugin artifact integrity failure"));
        }
        return Ok(());
    }
    let module = STANDARD.decode(&package.wasm).map_err(io::Error::other)?;
    if module.len() > MAX_MODULE || format!("{:x}", Sha256::digest(&module)) != package.sha256 {
        return Err(io::Error::other("plugin artifact integrity failure"));
    }
    let staging = store
        .plugins_path()
        .join(format!(".install-{}", uuid::Uuid::new_v4()));
    tokio::fs::create_dir(&staging).await?;
    let result = async {
        tokio::fs::write(staging.join("package.json"), bytes).await?;
        tokio::fs::write(
            staging.join("manifest.json"),
            serde_json::to_vec_pretty(&package.manifest).map_err(io::Error::other)?,
        )
        .await?;
        tokio::fs::write(staging.join("module.wasm"), module).await?;
        tokio::fs::rename(&staging, &destination).await
    }
    .await;
    if result.is_err() {
        let _ = clean_known(&staging).await;
    }
    result
}
pub async fn wasm(store: &IdentityStore, package: &Package) -> io::Result<String> {
    let directory = directory(store, &package.manifest.id)?;
    // Older DB-only installations are materialized lazily as well as at startup.
    if !tokio::fs::try_exists(&directory).await? {
        persist(store, package).await?;
    }
    let module = read(
        store.plugins_path(),
        &directory.join("module.wasm"),
        MAX_MODULE,
    )
    .await?;
    if format!("{:x}", Sha256::digest(&module)) != package.sha256 {
        return Err(io::Error::other("plugin module integrity failure"));
    }
    Ok(STANDARD.encode(module))
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
    let rows: Vec<String> = query_scalar("SELECT package_json FROM extensions ORDER BY id")
        .fetch_all(store.pool())
        .await
        .map_err(io::Error::other)?;
    let count = rows.len();
    for bytes in rows {
        let package: Package = serde_json::from_str(&bytes).map_err(io::Error::other)?;
        persist(store, &package).await?;
    }
    Ok(count)
}
#[cfg(test)]
mod tests {
    use super::*;
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
        let directory = store.plugins_path().join("independent.plugin");
        assert!(directory.join("manifest.json").exists());
        assert!(directory.join("package.json").exists());
        tokio::fs::write(directory.join("module.wasm"), b"tampered")
            .await
            .unwrap();
        assert!(wasm(&store, &package).await.is_err());
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
