use super::*;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Entry {
    pub(super) manifest: Manifest,
    pub(super) publisher_id: String,
    pub(super) public_key: String,
    pub(super) package_url: String,
    /// SHA-256 of the complete package JSON bytes, distinct from its Wasm digest.
    pub(super) package_sha256: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct LegacyManifest {
    pub(super) api_version: u32,
    pub(super) extensions: Vec<Entry>,
}

/// One plugin of a versioned catalogue. Unknown fields are ignored so that
/// repositories may carry presentation data for other clients.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PluginEntry {
    #[serde(alias = "guid")]
    pub(super) id: String,
    pub(super) name: String,
    #[serde(default)]
    pub(super) description: String,
    #[serde(default)]
    pub(super) overview: Option<String>,
    #[serde(default)]
    pub(super) owner: Option<String>,
    #[serde(default)]
    pub(super) category: Option<String>,
    #[serde(default)]
    pub(super) image_url: Option<String>,
    #[serde(default)]
    pub(super) publisher_id: Option<String>,
    #[serde(default)]
    pub(super) public_key: Option<String>,
    pub(super) versions: Vec<VersionEntry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct VersionEntry {
    pub(super) version: String,
    #[serde(default)]
    pub(super) target_abi: Option<String>,
    #[serde(default)]
    pub(super) min_server_version: Option<String>,
    #[serde(default)]
    pub(super) max_server_version: Option<String>,
    pub(super) source_url: String,
    /// SHA-256 of the exact `sourceUrl` bytes, hexadecimal (optionally `sha256:` prefixed).
    pub(super) checksum: String,
    #[serde(default)]
    pub(super) checksum_algorithm: Option<String>,
    #[serde(default)]
    pub(super) timestamp: Option<String>,
    #[serde(default)]
    pub(super) changelog: Option<String>,
    /// Optional presentation copy of the signed permissions; checked at install.
    #[serde(default)]
    pub(super) permissions: Vec<String>,
    #[serde(default)]
    pub(super) http_hosts: Vec<String>,
    /// Optional per-version publisher, for a plugin whose signing key changed.
    #[serde(default)]
    pub(super) publisher_id: Option<String>,
    #[serde(default)]
    pub(super) public_key: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct VersionedManifest {
    #[allow(dead_code)]
    pub(super) api_version: u32,
    pub(super) plugins: Vec<PluginEntry>,
}

pub(super) fn version(raw: &str) -> Option<[u64; 3]> {
    let parts: Vec<_> = raw.split('.').collect();
    if parts.len() != 3
        || parts.iter().any(|part| {
            part.is_empty() || part.len() > 8 || !part.bytes().all(|byte| byte.is_ascii_digit())
        })
    {
        return None;
    }
    Some([
        parts[0].parse().ok()?,
        parts[1].parse().ok()?,
        parts[2].parse().ok()?,
    ])
}

/// Numeric dotted ABI (`1`, `1.0`, `1.0.0.0`), at most four components.
pub(super) fn abi(raw: &str) -> Option<[u64; 4]> {
    let parts: Vec<_> = raw.split('.').collect();
    if parts.is_empty()
        || parts.len() > 4
        || parts.iter().any(|part| {
            part.is_empty() || part.len() > 8 || !part.bytes().all(|byte| byte.is_ascii_digit())
        })
    {
        return None;
    }
    let mut value = [0; 4];
    for (slot, part) in value.iter_mut().zip(parts) {
        *slot = part.parse().ok()?;
    }
    Some(value)
}

/// `targetAbi` is the minimum ABI an extension needs: same major, not newer than the host.
pub(super) fn abi_supported(raw: &str) -> bool {
    abi(raw).is_some_and(|target| target[0] == HOST_ABI[0] && target <= HOST_ABI)
}

pub(super) fn requirements_met(
    api_version: u32,
    target_abi: Option<&str>,
    min_server_version: Option<&str>,
    max_server_version: Option<&str>,
) -> bool {
    let current = media_contracts::version::current_version();
    let Some(current) = version(current.split('-').next().unwrap_or(current)) else {
        return false;
    };
    api_version == 1
        && target_abi.is_none_or(abi_supported)
        && min_server_version
            .is_none_or(|minimum| version(minimum).is_some_and(|minimum| current >= minimum))
        && max_server_version
            .is_none_or(|maximum| version(maximum).is_some_and(|maximum| current <= maximum))
}

pub(in super::super) fn compatible(manifest: &Manifest) -> bool {
    requirements_met(
        manifest.api_version,
        manifest.target_abi.as_deref(),
        manifest.min_server_version.as_deref(),
        manifest.max_server_version.as_deref(),
    )
}

pub(super) fn item_compatible(item: &Item) -> bool {
    match &item.manifest {
        Some(manifest) => compatible(manifest),
        None => requirements_met(
            1,
            item.target_abi.as_deref(),
            item.min_server_version.as_deref(),
            item.max_server_version.as_deref(),
        ),
    }
}

pub(super) fn invalid_manifest() -> Error {
    err(StatusCode::BAD_REQUEST, "invalid_repository_manifest")
}

pub(super) fn text(value: &str, maximum: usize) -> bool {
    value.chars().count() <= maximum
        && !value
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
}

pub(super) fn optional_text(value: &Option<String>, maximum: usize) -> bool {
    value.as_deref().is_none_or(|value| text(value, maximum))
}

pub(super) fn sha256_hex(raw: &str) -> Option<String> {
    let raw = raw
        .strip_prefix("sha256:")
        .unwrap_or(raw)
        .to_ascii_lowercase();
    (raw.len() == 64 && raw.bytes().all(|c| c.is_ascii_hexdigit())).then_some(raw)
}

pub(super) fn public_key(raw: &str) -> bool {
    raw.len() <= 64 && STANDARD.decode(raw).is_ok_and(|key| key.len() == 32)
}

pub(super) fn legacy_items(document: LegacyManifest) -> Result<Vec<Item>, Error> {
    if document.api_version != 1 || document.extensions.len() > 100 {
        return Err(invalid_manifest());
    }
    let mut items = Vec::with_capacity(document.extensions.len());
    for entry in document.extensions {
        let m = &entry.manifest;
        if !identifier(&m.id)
            || !identifier(&entry.publisher_id)
            || version(&m.version).is_none()
            || m.name.is_empty()
            || m.name.len() > 120
            || m.description.len() > 2048
            || !public_key(&entry.public_key)
            || entry.package_sha256.len() != 64
            || !entry
                .package_sha256
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        {
            return Err(invalid_manifest());
        }
        safe_url(&entry.package_url)?;
        items.push(Item {
            id: m.id.clone(),
            version: m.version.clone(),
            name: m.name.clone(),
            description: m.description.clone(),
            overview: None,
            owner: m.author.clone(),
            category: m.category.clone(),
            image_url: None,
            publisher_id: entry.publisher_id,
            public_key: entry.public_key,
            source_url: entry.package_url,
            checksum: entry.package_sha256,
            target_abi: m.target_abi.clone(),
            min_server_version: m.min_server_version.clone(),
            max_server_version: m.max_server_version.clone(),
            timestamp: m.release_date.clone(),
            changelog: m.changelog.clone(),
            permissions: m.permissions.clone(),
            http_hosts: m.http_hosts.clone(),
            manifest: Some(entry.manifest),
        });
    }
    Ok(items)
}

pub(super) fn versioned_items(plugins: Vec<PluginEntry>) -> Result<Vec<Item>, Error> {
    if plugins.len() > MAX_PLUGINS
        || plugins
            .iter()
            .map(|plugin| plugin.versions.len())
            .sum::<usize>()
            > MAX_VERSIONS
    {
        return Err(invalid_manifest());
    }
    let mut ids = HashSet::new();
    let mut items = Vec::new();
    for plugin in plugins {
        if !identifier(&plugin.id)
            || !ids.insert(plugin.id.clone())
            || plugin.name.trim().is_empty()
            || !text(&plugin.name, 120)
            || !text(&plugin.description, 2048)
            || !optional_text(&plugin.overview, 8192)
            || !optional_text(&plugin.owner, 120)
            || !optional_text(&plugin.category, 60)
        {
            return Err(invalid_manifest());
        }
        if let Some(image) = &plugin.image_url {
            safe_url(image)?;
        }
        for entry in plugin.versions {
            let publisher_id = entry
                .publisher_id
                .or_else(|| plugin.publisher_id.clone())
                .ok_or_else(invalid_manifest)?;
            let key = entry
                .public_key
                .or_else(|| plugin.public_key.clone())
                .ok_or_else(invalid_manifest)?;
            // Integrity is SHA-256 only. A Jellyfin-style MD5 is not a substitute.
            let checksum = entry
                .checksum_algorithm
                .as_deref()
                .is_none_or(|algorithm| algorithm.eq_ignore_ascii_case("sha256"))
                .then(|| sha256_hex(&entry.checksum))
                .flatten()
                .ok_or_else(invalid_manifest)?;
            if version(&entry.version).is_none()
                || entry
                    .target_abi
                    .as_deref()
                    .is_some_and(|raw| abi(raw).is_none())
                || entry
                    .min_server_version
                    .as_deref()
                    .is_some_and(|raw| version(raw).is_none())
                || entry
                    .max_server_version
                    .as_deref()
                    .is_some_and(|raw| version(raw).is_none())
                || !optional_text(&entry.timestamp, 64)
                || !optional_text(&entry.changelog, 8192)
                || entry.permissions.len() > 32
                || entry
                    .permissions
                    .iter()
                    .any(|p| p.is_empty() || !text(p, 64))
                || entry.http_hosts.len() > 16
                || entry
                    .http_hosts
                    .iter()
                    .any(|host| !super::super::http_host(host))
                || !identifier(&publisher_id)
                || !public_key(&key)
            {
                return Err(invalid_manifest());
            }
            safe_url(&entry.source_url)?;
            items.push(Item {
                id: plugin.id.clone(),
                version: entry.version,
                name: plugin.name.clone(),
                description: plugin.description.clone(),
                overview: plugin.overview.clone(),
                owner: plugin.owner.clone().unwrap_or_default(),
                category: plugin.category.clone(),
                image_url: plugin.image_url.clone(),
                publisher_id,
                public_key: key,
                source_url: entry.source_url,
                checksum,
                target_abi: entry.target_abi,
                min_server_version: entry.min_server_version,
                max_server_version: entry.max_server_version,
                timestamp: entry.timestamp,
                changelog: entry.changelog,
                permissions: entry.permissions,
                http_hosts: entry.http_hosts,
                manifest: None,
            });
        }
    }
    Ok(items)
}

pub(super) fn parse_manifest(bytes: &[u8]) -> Result<Vec<Item>, Error> {
    let document: Value = serde_json::from_slice(bytes).map_err(|_| invalid_manifest())?;
    let items = match document
        .as_object()
        .map(|object| object.get("apiVersion").and_then(Value::as_u64))
    {
        // A bare array is a Jellyfin-style list of plugins.
        None if document.is_array() => {
            versioned_items(serde_json::from_value(document).map_err(|_| invalid_manifest())?)?
        }
        Some(Some(1)) => {
            legacy_items(serde_json::from_value(document).map_err(|_| invalid_manifest())?)?
        }
        Some(Some(2)) => versioned_items(
            serde_json::from_value::<VersionedManifest>(document)
                .map_err(|_| invalid_manifest())?
                .plugins,
        )?,
        _ => return Err(invalid_manifest()),
    };
    let mut unique = HashSet::new();
    if items
        .iter()
        .any(|item| !unique.insert((item.id.as_str(), item.version.as_str())))
    {
        return Err(invalid_manifest());
    }
    Ok(items)
}

/// Manifest-shaped presentation of a catalogue version, for the Web UI.
pub(super) fn display_manifest(item: &Item) -> Value {
    if let Some(manifest) = &item.manifest {
        return serde_json::to_value(manifest).unwrap_or(Value::Null);
    }
    json!({
        "apiVersion": 1,
        "id": item.id,
        "name": item.name,
        "version": item.version,
        "description": item.description,
        "author": item.owner,
        "license": "",
        "repository": "",
        "permissions": item.permissions,
        "httpHosts": item.http_hosts,
        "category": item.category,
        "targetAbi": item.target_abi,
        "minServerVersion": item.min_server_version,
        "maxServerVersion": item.max_server_version,
        "changelog": item.changelog,
        "releaseDate": item.timestamp,
    })
}

/// Extract the signed package JSON from a release asset: the JSON itself or a
/// zip archive containing exactly one `*.package.json` (stored or deflated).
pub(super) fn package_bytes(asset: &[u8]) -> Result<Vec<u8>, Error> {
    let invalid = || err(StatusCode::BAD_REQUEST, "invalid_package_archive");
    if !asset.starts_with(b"PK\x03\x04") {
        if asset.len() > MAX_PACKAGE {
            return Err(err(StatusCode::BAD_REQUEST, "package_too_large"));
        }
        return Ok(asset.to_vec());
    }
    let u16_at = |offset: usize| {
        asset
            .get(offset..offset.checked_add(2)?)
            .map(|b| usize::from(u16::from_le_bytes([b[0], b[1]])))
    };
    let u32_at = |offset: usize| {
        asset
            .get(offset..offset.checked_add(4)?)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    // End of central directory: within the trailing 22 bytes plus a 64 KiB comment.
    let last = asset.len().checked_sub(22).ok_or_else(invalid)?;
    let first = asset.len().saturating_sub(22 + 65_535);
    let eocd = (first..=last)
        .rev()
        .find(|&offset| asset[offset..].starts_with(b"PK\x05\x06"))
        .ok_or_else(invalid)?;
    let entries = u16_at(eocd + 10).ok_or_else(invalid)?;
    let directory_size = u32_at(eocd + 12).ok_or_else(invalid)? as usize;
    let directory = u32_at(eocd + 16).ok_or_else(invalid)? as usize;
    if u16_at(eocd + 4) != Some(0)
        || u16_at(eocd + 6) != Some(0)
        || u16_at(eocd + 8) != Some(entries)
        || entries == 0
        || entries > 64
        || directory
            .checked_add(directory_size)
            .is_none_or(|end| end > eocd)
    {
        return Err(invalid());
    }
    let mut cursor = directory;
    let mut found = None;
    for _ in 0..entries {
        if !asset
            .get(cursor..)
            .is_some_and(|rest| rest.starts_with(b"PK\x01\x02"))
        {
            return Err(invalid());
        }
        let field = |offset| u16_at(cursor + offset).ok_or_else(invalid);
        let wide = |offset| u32_at(cursor + offset).ok_or_else(invalid);
        let (flags, method, crc) = (field(8)?, field(10)?, wide(16)?);
        let (compressed, size, local) = (wide(20)?, wide(24)?, wide(42)?);
        let (name_length, extra, comment) = (field(28)?, field(30)?, field(32)?);
        let name = asset
            .get(cursor + 46..cursor + 46 + name_length)
            .ok_or_else(invalid)?;
        cursor += 46 + name_length + extra + comment;
        let Ok(name) = std::str::from_utf8(name) else {
            continue;
        };
        let file = name.rsplit('/').next().unwrap_or(name).to_ascii_lowercase();
        if name.ends_with('/') || name.starts_with("__MACOSX/") || !file.ends_with(".package.json")
        {
            continue;
        }
        if found.is_some() {
            return Err(err(StatusCode::BAD_REQUEST, "package_archive_ambiguous"));
        }
        found = Some((flags, method, crc, compressed, size, local));
    }
    let (flags, method, crc, compressed, size, local) =
        found.ok_or_else(|| err(StatusCode::BAD_REQUEST, "package_archive_without_package"))?;
    // Encrypted and ZIP64 entries are not supported; the declared size is bounded first.
    if flags & 1 != 0 || compressed == u32::MAX || size == u32::MAX {
        return Err(invalid());
    }
    let (compressed, size) = (compressed as usize, size as usize);
    if size > MAX_PACKAGE {
        return Err(err(StatusCode::BAD_REQUEST, "package_too_large"));
    }
    let local = local as usize;
    if !asset
        .get(local..)
        .is_some_and(|rest| rest.starts_with(b"PK\x03\x04"))
    {
        return Err(invalid());
    }
    let start = local
        + 30
        + u16_at(local + 26).ok_or_else(invalid)?
        + u16_at(local + 28).ok_or_else(invalid)?;
    let data = asset
        .get(start..start.checked_add(compressed).ok_or_else(invalid)?)
        .ok_or_else(invalid)?;
    let bytes = match method {
        0 if compressed == size => data.to_vec(),
        8 => {
            let mut output = Vec::with_capacity(size);
            flate2::read::DeflateDecoder::new(data)
                .take(MAX_PACKAGE as u64 + 1)
                .read_to_end(&mut output)
                .map_err(|_| invalid())?;
            output
        }
        _ => return Err(invalid()),
    };
    let mut digest = flate2::Crc::new();
    digest.update(&bytes);
    if bytes.len() != size || digest.sum() != crc {
        return Err(invalid());
    }
    Ok(bytes)
}

pub(super) fn same_set(declared: &[String], signed: &[String]) -> bool {
    declared.iter().all(|value| signed.contains(value))
        && signed.iter().all(|value| declared.contains(value))
}
