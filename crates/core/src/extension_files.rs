//! Optional administrator-managed signed-package inbox. Never grants libraries
//! or activates extensions, and never overwrites an existing publisher or package.
use crate::{
    database::{query, query_scalar},
    extensions::validate_package,
    identity::IdentityStore,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use media_extension_sdk::Package;
use serde::Deserialize;
use std::{io, path::Path};
use tokio::io::AsyncReadExt;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Publisher {
    id: String,
    public_key: String,
}
async fn read(root: &Path, path: &Path) -> io::Result<Vec<u8>> {
    let opened = media_node::OpenedFile::open_in_root(root, path).await?;
    if opened.size() > 400000 {
        return Err(io::Error::other("extension import file too large"));
    }
    let mut bytes = Vec::new();
    tokio::fs::File::from_std(opened.try_clone_std()?)
        .take(400001)
        .read_to_end(&mut bytes)
        .await?;
    if bytes.len() > 400000 {
        return Err(io::Error::other("extension import file too large"));
    }
    opened.verify_unchanged_sync()?;
    Ok(bytes)
}
pub async fn import(store: &IdentityStore, root: &Path) -> io::Result<usize> {
    let root = root.canonicalize()?;
    // This explicit local trust file is separate from the packages it approves.
    // Dropping only a package never makes its signer trusted.
    match read(&root, Path::new("trusted-publishers.json")).await {
        Ok(bytes) => {
            let publishers: Vec<Publisher> = serde_json::from_slice(&bytes)
                .map_err(|_| io::Error::other("invalid local publisher trust file"))?;
            if publishers.len() > 32 {
                return Err(io::Error::other("local publisher limit"));
            }
            for publisher in publishers {
                if publisher.id.is_empty()
                    || publisher.id.len() > 80
                    || !publisher
                        .id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
                    || STANDARD
                        .decode(&publisher.public_key)
                        .map_err(io::Error::other)?
                        .len()
                        != 32
                {
                    return Err(io::Error::other("invalid local publisher"));
                }
                query("INSERT INTO extension_publishers(id,public_key,active) VALUES(?,?,1) ON CONFLICT DO NOTHING").bind(publisher.id).bind(publisher.public_key).execute(&store.pool).await.map_err(io::Error::other)?;
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let mut files = tokio::fs::read_dir(&root).await?;
    let mut imported = 0;
    let mut seen = 0;
    while let Some(file) = files.next_entry().await? {
        if !file
            .file_name()
            .to_string_lossy()
            .ends_with(".package.json")
        {
            continue;
        }
        seen += 1;
        if seen > 32 {
            return Err(io::Error::other("local extension import limit"));
        }
        let bytes = read(&root, Path::new(&file.file_name())).await?;
        let package: Package = serde_json::from_slice(&bytes)
            .map_err(|_| io::Error::other("invalid local extension package"))?;
        let key: Option<String> =
            query_scalar("SELECT public_key FROM extension_publishers WHERE id=? AND active=1")
                .bind(&package.publisher_id)
                .fetch_optional(&store.pool)
                .await
                .map_err(io::Error::other)?;
        let key =
            key.ok_or_else(|| io::Error::other("local extension signer is untrusted or revoked"))?;
        validate_package(&package, &STANDARD.decode(key).map_err(io::Error::other)?)
            .map_err(io::Error::other)?;
        let mut conn = store.pool.acquire().await.map_err(io::Error::other)?;
        let mut tx = crate::database::begin_write(&mut conn)
            .await
            .map_err(io::Error::other)?;
        let count: i64 = query_scalar("SELECT COUNT(*) FROM extensions")
            .fetch_one(&mut *tx)
            .await
            .map_err(io::Error::other)?;
        let existing: i64 = query_scalar("SELECT COUNT(*) FROM extensions WHERE id=?")
            .bind(&package.manifest.id)
            .fetch_one(&mut *tx)
            .await
            .map_err(io::Error::other)?;
        if count >= 32 && existing == 0 {
            return Err(io::Error::other("installed extension limit"));
        }
        let result=query("INSERT INTO extensions(id,publisher_id,package_json,generation) VALUES(?,?,?,?) ON CONFLICT DO NOTHING").bind(&package.manifest.id).bind(&package.publisher_id).bind(String::from_utf8(bytes).map_err(io::Error::other)?).bind(uuid::Uuid::new_v4().to_string()).execute(&mut *tx).await.map_err(io::Error::other)?;
        if result.rows_affected() == 1 {
            query("INSERT INTO extension_audit(id,actor_id,extension_id,action,created_at) VALUES(?,'local-file-import',?,'installed-disabled-from-files',?)").bind(uuid::Uuid::new_v4().to_string()).bind(&package.manifest.id).bind(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64).execute(&mut *tx).await.map_err(io::Error::other)?;
            imported += 1;
        }
        tx.commit().await.map_err(io::Error::other)?;
    }
    Ok(imported)
}
