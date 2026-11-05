//! Explicit administrator-owned HTTPS catalogues. A catalogue never grants publisher trust.
//!
//! Two catalogue formats are accepted:
//! - `apiVersion: 1`: one entry per signed package, pinning its complete manifest.
//! - `apiVersion: 2` (or a bare JSON array of plugins): one entry per plugin with a
//!   `versions` array, in the style of a Jellyfin plugin repository. Every version
//!   names a release asset (`sourceUrl`) and its SHA-256 (`checksum`). The asset is
//!   the signed `.package.json` or a `.zip` containing it; the Ed25519 signature of
//!   the package remains the only source of trust.
use super::{Error, admin, db, err, identifier, now};
use crate::{
    auth::AuthState,
    database::{query, query_as, query_scalar},
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::IntoResponse,
    routing::{get, post},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use media_extension_sdk::{Manifest, Package};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    io::Read,
    net::{IpAddr, SocketAddr},
    time::{Duration, Instant},
};
use tokio::sync::Semaphore;

const MAX_MANIFEST: usize = 1024 * 1024;
/// A downloaded release asset: the signed package JSON or a zip that contains it.
const MAX_DOWNLOAD: usize = 8 * 1024 * 1024;
/// The extracted signed package JSON. The Wasm module limit keeps real packages far below.
const MAX_PACKAGE: usize = 2 * 1024 * 1024;
const MAX_IMAGE: usize = 512 * 1024;
const MAX_REDIRECTS: usize = 3;
const MAX_PLUGINS: usize = 200;
const MAX_VERSIONS: usize = 1000;
/// Extension ABI implemented by this Core. A catalogue `targetAbi` is a minimum.
const HOST_ABI: [u64; 4] = [1, 0, 0, 0];
static DOWNLOADS: Semaphore = Semaphore::const_new(2);
static IMAGES: Semaphore = Semaphore::const_new(4);

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Entry {
    manifest: Manifest,
    publisher_id: String,
    public_key: String,
    package_url: String,
    /// SHA-256 of the complete package JSON bytes, distinct from its Wasm digest.
    package_sha256: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LegacyManifest {
    api_version: u32,
    extensions: Vec<Entry>,
}
/// One plugin of a versioned catalogue. Unknown fields are ignored so that
/// repositories may carry presentation data for other clients.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginEntry {
    #[serde(alias = "guid")]
    id: String,
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    overview: Option<String>,
    #[serde(default)]
    owner: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    image_url: Option<String>,
    #[serde(default)]
    publisher_id: Option<String>,
    #[serde(default)]
    public_key: Option<String>,
    versions: Vec<VersionEntry>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct VersionEntry {
    version: String,
    #[serde(default)]
    target_abi: Option<String>,
    #[serde(default)]
    min_server_version: Option<String>,
    #[serde(default)]
    max_server_version: Option<String>,
    source_url: String,
    /// SHA-256 of the exact `sourceUrl` bytes, hexadecimal (optionally `sha256:` prefixed).
    checksum: String,
    #[serde(default)]
    checksum_algorithm: Option<String>,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default)]
    changelog: Option<String>,
    /// Optional presentation copy of the signed permissions; checked at install.
    #[serde(default)]
    permissions: Vec<String>,
    #[serde(default)]
    http_hosts: Vec<String>,
    /// Optional per-version publisher, for a plugin whose signing key changed.
    #[serde(default)]
    publisher_id: Option<String>,
    #[serde(default)]
    public_key: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct VersionedManifest {
    #[allow(dead_code)]
    api_version: u32,
    plugins: Vec<PluginEntry>,
}

/// One installable (extension, version) of a catalogue, whatever its format.
#[derive(Clone)]
struct Item {
    id: String,
    version: String,
    name: String,
    description: String,
    overview: Option<String>,
    owner: String,
    category: Option<String>,
    image_url: Option<String>,
    publisher_id: String,
    public_key: String,
    source_url: String,
    checksum: String,
    target_abi: Option<String>,
    min_server_version: Option<String>,
    max_server_version: Option<String>,
    timestamp: Option<String>,
    changelog: Option<String>,
    permissions: Vec<String>,
    http_hosts: Vec<String>,
    /// `apiVersion: 1` entries pin the complete signed manifest.
    manifest: Option<Manifest>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RepositoryInput {
    name: String,
    url: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InstallInput {
    extension_id: String,
    version: String,
    #[serde(default)]
    upgrade: bool,
    #[serde(default)]
    confirm_downgrade: bool,
    expected_version: Option<String>,
    expected_sha256: Option<String>,
}

pub(super) fn router() -> Router<AuthState> {
    Router::new()
        .route("/api/v1/admin/extensions/repositories", get(list).post(add))
        .route(
            "/api/v1/admin/extensions/repositories/{id}",
            axum::routing::delete(remove),
        )
        .route(
            "/api/v1/admin/extensions/repositories/{id}/refresh",
            post(refresh),
        )
        .route(
            "/api/v1/admin/extensions/repositories/{id}/install",
            post(install),
        )
        .route(
            "/api/v1/admin/extensions/repositories/{id}/image/{extension}",
            get(image),
        )
        .route("/api/v1/admin/extensions/catalogue", get(catalogue))
}

fn checked_url(raw: &str, maximum: usize) -> Result<url::Url, Error> {
    let url =
        url::Url::parse(raw).map_err(|_| err(StatusCode::BAD_REQUEST, "invalid_repository_url"))?;
    if raw.len() > maximum
        || url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.port_or_known_default() != Some(443)
        || !matches!(url.host(),Some(url::Host::Domain(host)) if !host.eq_ignore_ascii_case("localhost") && !host.ends_with(".localhost"))
    {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "repository_https_public_host_required",
        ));
    }
    Ok(url)
}
fn safe_url(raw: &str) -> Result<url::Url, Error> {
    checked_url(raw, 2048)
}
/// Accept the public GitHub pages administrators copy from their browser, while
/// storing the actual JSON endpoint. Package and redirect URL rules are unchanged.
fn catalogue_url(raw: &str) -> Result<url::Url, Error> {
    let url = safe_url(raw)?;
    if url.host_str() != Some("github.com") || url.query().is_some() {
        return Ok(url);
    }
    let segments: Vec<_> = url.path().trim_matches('/').split('/').collect();
    let (owner, repository, branch, path) = match segments.as_slice() {
        [owner, repository] => (
            *owner,
            *repository,
            "main",
            "repository/manifest.json".into(),
        ),
        [owner, repository, "tree", branch] => (
            *owner,
            *repository,
            *branch,
            "repository/manifest.json".into(),
        ),
        [owner, repository, "blob", branch, path @ ..] if !path.is_empty() => {
            (*owner, *repository, *branch, path.join("/"))
        }
        _ => return Ok(url),
    };
    safe_url(&format!(
        "https://raw.githubusercontent.com/{owner}/{repository}/{branch}/{path}"
    ))
}
/// GitHub serves release assets through a redirect to its content hosts. Any
/// other cross-host redirect is refused; every hop is validated again.
fn redirect_allowed(from: &url::Url, to: &url::Url) -> bool {
    let (Some(from), Some(to)) = (from.host_str(), to.host_str()) else {
        return false;
    };
    from.eq_ignore_ascii_case(to)
        || (from.eq_ignore_ascii_case("github.com")
            && [
                "objects.githubusercontent.com",
                "release-assets.githubusercontent.com",
                "codeload.github.com",
            ]
            .iter()
            .any(|host| to.eq_ignore_ascii_case(host)))
}
pub(crate) fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => {
            let [a, b, _, _] = v.octets();
            !v.is_private()
                && !v.is_loopback()
                && !v.is_link_local()
                && !v.is_unspecified()
                && !v.is_broadcast()
                && !v.is_documentation()
                && !v.is_multicast()
                && a != 0
                && a < 224
                && !(a == 100 && (64..=127).contains(&b))
                && !(a == 198 && (b == 18 || b == 19))
        }
        // Permit only global unicast; reject mapped IPv4, ULA, local and documentation ranges.
        IpAddr::V6(v) => {
            let s = v.segments();
            (s[0] & 0xe000) == 0x2000
                && s[0] != 0x2002
                && !(s[0] == 0x2001 && (s[1] <= 0x1ff || s[1] == 0xdb8))
        }
    }
}
#[cfg(test)]
fn test_document(raw: &str, maximum: usize) -> Option<Result<Vec<u8>, Error>> {
    let bytes = test_documents().lock().unwrap().get(raw).cloned()?;
    Some(if bytes.len() <= maximum {
        Ok(bytes)
    } else {
        Err(err(
            StatusCode::BAD_GATEWAY,
            "repository_response_too_large",
        ))
    })
}
fn user_agent() -> String {
    format!(
        "Aether-Extensions/{}",
        media_contracts::version::current_version()
    )
}
/// Bounded HTTPS GET of a public document (catalogue, GitHub API, collection files).
pub(crate) async fn fetch(raw: &str, maximum: usize) -> Result<Vec<u8>, Error> {
    safe_url(raw)?;
    #[cfg(test)]
    if let Some(result) = test_document(raw, maximum) {
        return result;
    }
    let _permit = DOWNLOADS
        .try_acquire()
        .map_err(|_| err(StatusCode::TOO_MANY_REQUESTS, "repository_busy"))?;
    download(raw, maximum, "application/json", Duration::from_secs(10)).await
}
async fn fetch_asset(raw: &str, maximum: usize) -> Result<Vec<u8>, Error> {
    safe_url(raw)?;
    #[cfg(test)]
    if let Some(result) = test_document(raw, maximum) {
        return result;
    }
    let _permit = DOWNLOADS
        .try_acquire()
        .map_err(|_| err(StatusCode::TOO_MANY_REQUESTS, "repository_busy"))?;
    download(raw, maximum, "*/*", Duration::from_secs(60)).await
}
async fn download(
    raw: &str,
    maximum: usize,
    accept: &str,
    timeout: Duration,
) -> Result<Vec<u8>, Error> {
    let mut url = safe_url(raw)?;
    for hop in 0..=MAX_REDIRECTS {
        let host = url
            .host_str()
            .ok_or_else(|| err(StatusCode::BAD_REQUEST, "invalid_repository_url"))?
            .to_owned();
        let addresses: Vec<SocketAddr> = tokio::time::timeout(
            Duration::from_secs(3),
            tokio::net::lookup_host((host.as_str(), 443)),
        )
        .await
        .map_err(|_| err(StatusCode::BAD_GATEWAY, "repository_unavailable"))?
        .map_err(|_| err(StatusCode::BAD_GATEWAY, "repository_unavailable"))?
        .collect();
        if addresses.is_empty()
            || addresses.len() > 32
            || addresses.iter().any(|address| !public_ip(address.ip()))
        {
            return Err(err(StatusCode::BAD_REQUEST, "repository_address_forbidden"));
        }
        // Redirects are followed by hand: each hop is pinned to its validated addresses.
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(timeout)
            .connect_timeout(Duration::from_secs(3))
            .resolve_to_addrs(&host, &addresses)
            .build()
            .map_err(|_| err(StatusCode::BAD_GATEWAY, "repository_unavailable"))?;
        let mut response = client
            .get(url.clone())
            .header("accept", accept)
            .header("user-agent", user_agent())
            .send()
            .await
            .map_err(|_| err(StatusCode::BAD_GATEWAY, "repository_unavailable"))?;
        if response.status().is_redirection() {
            let next = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|location| location.to_str().ok())
                .and_then(|location| url.join(location).ok())
                .ok_or_else(|| err(StatusCode::BAD_GATEWAY, "repository_redirect_forbidden"))?;
            // Signed asset URLs carry long query strings; the host policy is unchanged.
            let next = checked_url(next.as_str(), 8192)
                .map_err(|_| err(StatusCode::BAD_GATEWAY, "repository_redirect_forbidden"))?;
            if hop == MAX_REDIRECTS || !redirect_allowed(&url, &next) {
                return Err(err(
                    StatusCode::BAD_GATEWAY,
                    "repository_redirect_forbidden",
                ));
            }
            url = next;
            continue;
        }
        if !response.status().is_success() {
            return Err(err(StatusCode::BAD_GATEWAY, "repository_http_error"));
        }
        if response
            .content_length()
            .is_some_and(|size| size > maximum as u64)
        {
            return Err(err(
                StatusCode::BAD_GATEWAY,
                "repository_response_too_large",
            ));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| err(StatusCode::BAD_GATEWAY, "repository_unavailable"))?
        {
            if bytes.len().saturating_add(chunk.len()) > maximum {
                return Err(err(
                    StatusCode::BAD_GATEWAY,
                    "repository_response_too_large",
                ));
            }
            bytes.extend_from_slice(&chunk);
        }
        return Ok(bytes);
    }
    Err(err(
        StatusCode::BAD_GATEWAY,
        "repository_redirect_forbidden",
    ))
}
fn version(raw: &str) -> Option<[u64; 3]> {
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
fn abi(raw: &str) -> Option<[u64; 4]> {
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
fn abi_supported(raw: &str) -> bool {
    abi(raw).is_some_and(|target| target[0] == HOST_ABI[0] && target <= HOST_ABI)
}
fn requirements_met(
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
pub(super) fn compatible(manifest: &Manifest) -> bool {
    requirements_met(
        manifest.api_version,
        manifest.target_abi.as_deref(),
        manifest.min_server_version.as_deref(),
        manifest.max_server_version.as_deref(),
    )
}
fn item_compatible(item: &Item) -> bool {
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
fn invalid_manifest() -> Error {
    err(StatusCode::BAD_REQUEST, "invalid_repository_manifest")
}
fn text(value: &str, maximum: usize) -> bool {
    value.chars().count() <= maximum
        && !value
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
}
fn optional_text(value: &Option<String>, maximum: usize) -> bool {
    value.as_deref().is_none_or(|value| text(value, maximum))
}
fn sha256_hex(raw: &str) -> Option<String> {
    let raw = raw
        .strip_prefix("sha256:")
        .unwrap_or(raw)
        .to_ascii_lowercase();
    (raw.len() == 64 && raw.bytes().all(|c| c.is_ascii_hexdigit())).then_some(raw)
}
fn public_key(raw: &str) -> bool {
    raw.len() <= 64 && STANDARD.decode(raw).is_ok_and(|key| key.len() == 32)
}
fn legacy_items(document: LegacyManifest) -> Result<Vec<Item>, Error> {
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
fn versioned_items(plugins: Vec<PluginEntry>) -> Result<Vec<Item>, Error> {
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
                || entry.http_hosts.iter().any(|host| !super::http_host(host))
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
fn parse_manifest(bytes: &[u8]) -> Result<Vec<Item>, Error> {
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
fn display_manifest(item: &Item) -> Value {
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
fn package_bytes(asset: &[u8]) -> Result<Vec<u8>, Error> {
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
fn same_set(declared: &[String], signed: &[String]) -> bool {
    declared.iter().all(|value| signed.contains(value))
        && signed.iter().all(|value| declared.contains(value))
}
async fn list(State(auth): State<AuthState>, headers: HeaderMap) -> Result<Json<Value>, Error> {
    admin(&auth, &headers, false).await?;
    let rows: Vec<(String, String, String, i64)> =
        query_as("SELECT id,name,url,refreshed_at FROM extension_repositories ORDER BY name,id")
            .fetch_all(auth.store.pool())
            .await
            .map_err(db)?;
    Ok(Json(
        json!({"repositories":rows.into_iter().map(|(id,name,url,time)|json!({"id":id,"name":name,"url":url,"refreshedAt":time})).collect::<Vec<_>>()}),
    ))
}
async fn add(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<RepositoryInput>,
) -> Result<(StatusCode, Json<Value>), Error> {
    admin(&auth, &headers, true).await?;
    let name = input.name.trim();
    let url = catalogue_url(input.url.trim())?.to_string();
    if name.is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) {
        return Err(err(StatusCode::BAD_REQUEST, "invalid_repository_name"));
    }
    let bytes = fetch(&url, MAX_MANIFEST).await?;
    parse_manifest(&bytes)?;
    admin(&auth, &headers, true).await?;
    let _lock = super::MUTATIONS.lock().await;
    let count: i64 = query_scalar("SELECT COUNT(*) FROM extension_repositories")
        .fetch_one(auth.store.pool())
        .await
        .map_err(db)?;
    if count >= 16 {
        return Err(err(StatusCode::CONFLICT, "repository_limit"));
    }
    let id = uuid::Uuid::new_v4().to_string();
    query("INSERT INTO extension_repositories(id,name,url,manifest_json,refreshed_at) VALUES(?,?,?,?,?)")
        .bind(&id).bind(name).bind(url).bind(String::from_utf8(bytes).map_err(|_|err(StatusCode::BAD_REQUEST,"invalid_repository_manifest"))?).bind(now())
        .execute(auth.store.pool()).await.map_err(|_|err(StatusCode::CONFLICT,"repository_exists"))?;
    Ok((StatusCode::CREATED, Json(json!({"id":id}))))
}
async fn refresh(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, Error> {
    admin(&auth, &headers, true).await?;
    let url: Option<String> = query_scalar("SELECT url FROM extension_repositories WHERE id=?")
        .bind(&id)
        .fetch_optional(auth.store.pool())
        .await
        .map_err(db)?;
    let bytes = fetch(
        &url.ok_or_else(|| err(StatusCode::NOT_FOUND, "unknown_repository"))?,
        MAX_MANIFEST,
    )
    .await?;
    parse_manifest(&bytes)?;
    admin(&auth, &headers, true).await?;
    let changed =
        query("UPDATE extension_repositories SET manifest_json=?,refreshed_at=? WHERE id=?")
            .bind(
                String::from_utf8(bytes)
                    .map_err(|_| err(StatusCode::BAD_REQUEST, "invalid_repository_manifest"))?,
            )
            .bind(now())
            .bind(id)
            .execute(auth.store.pool())
            .await
            .map_err(db)?;
    if changed.rows_affected() != 1 {
        return Err(err(StatusCode::NOT_FOUND, "unknown_repository"));
    }
    Ok(StatusCode::NO_CONTENT)
}
async fn remove(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, Error> {
    admin(&auth, &headers, true).await?;
    query("DELETE FROM extension_repositories WHERE id=?")
        .bind(id)
        .execute(auth.store.pool())
        .await
        .map_err(db)?;
    Ok(StatusCode::NO_CONTENT)
}
async fn catalogue(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Result<Json<Value>, Error> {
    admin(&auth, &headers, false).await?;
    let rows: Vec<(String, String, String)> =
        query_as("SELECT id,name,manifest_json FROM extension_repositories ORDER BY name,id")
            .fetch_all(auth.store.pool())
            .await
            .map_err(db)?;
    let mut entries = Vec::new();
    for (id, name, bytes) in rows {
        // One entry per published version, so earlier versions stay installable.
        for item in parse_manifest(bytes.as_bytes())? {
            // Avatars and Poster Rotator are native built-in features in Aether Core.
            // Do not expose them in the extensions catalogue.
            if matches!(item.id.as_str(), "avatars" | "poster-rotator") {
                continue;
            }
            entries.push(json!({
                "repositoryId": id,
                "repositoryName": name,
                "compatible": item_compatible(&item),
                "manifest": display_manifest(&item),
                "publisherId": item.publisher_id,
                "publicKey": item.public_key,
                "version": item.version,
                "overview": item.overview,
                "owner": item.owner,
                "hasImage": item.image_url.is_some(),
                "imageUrl": item.image_url,
                "sourceUrl": item.source_url,
                "timestamp": item.timestamp,
                "changelog": item.changelog,
            }));
        }
    }
    Ok(Json(json!({"entries":entries})))
}
type ImageCache = std::sync::Mutex<std::collections::HashMap<String, (Instant, String)>>;
fn image_cache() -> &'static ImageCache {
    static CACHE: std::sync::LazyLock<ImageCache> = std::sync::LazyLock::new(Default::default);
    &CACHE
}
/// Image types accepted from a catalogue `imageUrl`, recognised by content.
fn image_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.len() > 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        let text = std::str::from_utf8(bytes).ok()?;
        let text = text.trim_start_matches('\u{feff}').trim_start();
        (text.starts_with('<') && text.contains("<svg")).then_some("image/svg+xml")
    }
}
/// The catalogue image as a `data:` URL. The Web UI only loads same-origin or
/// `data:` images, and a JSON response is never rendered as a document.
async fn image(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path((id, extension)): Path<(String, String)>,
) -> Result<impl IntoResponse, Error> {
    admin(&auth, &headers, false).await?;
    let snapshot: Option<String> =
        query_scalar("SELECT manifest_json FROM extension_repositories WHERE id=?")
            .bind(&id)
            .fetch_optional(auth.store.pool())
            .await
            .map_err(db)?;
    let snapshot = snapshot.ok_or_else(|| err(StatusCode::NOT_FOUND, "unknown_repository"))?;
    let url = parse_manifest(snapshot.as_bytes())?
        .into_iter()
        .find(|item| item.id == extension)
        .and_then(|item| item.image_url)
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "extension_image_not_found"))?;
    let cached = image_cache()
        .lock()
        .ok()
        .and_then(|cache| cache.get(&url).cloned())
        .filter(|(time, _)| time.elapsed() < Duration::from_secs(3600))
        .map(|(_, data)| data);
    let data = match cached {
        Some(data) => data,
        None => {
            safe_url(&url)?;
            #[cfg(test)]
            let test = test_document(&url, MAX_IMAGE);
            #[cfg(not(test))]
            let test: Option<Result<Vec<u8>, Error>> = None;
            let bytes = match test {
                Some(result) => result?,
                None => {
                    let _permit = IMAGES
                        .acquire()
                        .await
                        .map_err(|_| err(StatusCode::SERVICE_UNAVAILABLE, "repository_busy"))?;
                    download(&url, MAX_IMAGE, "image/*", Duration::from_secs(10)).await?
                }
            };
            let kind = image_type(&bytes)
                .ok_or_else(|| err(StatusCode::BAD_GATEWAY, "extension_image_invalid"))?;
            let data = format!("data:{kind};base64,{}", STANDARD.encode(&bytes));
            if let Ok(mut cache) = image_cache().lock() {
                if cache.len() >= 128 {
                    cache.clear();
                }
                cache.insert(url, (Instant::now(), data.clone()));
            }
            data
        }
    };
    Ok((
        [(header::CACHE_CONTROL, "private, max-age=3600")],
        Json(json!({"dataUrl": data})),
    ))
}
async fn install(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<InstallInput>,
) -> Result<(StatusCode, Json<Value>), Error> {
    admin(&auth, &headers, true).await?;
    let snapshot: Option<String> =
        query_scalar("SELECT manifest_json FROM extension_repositories WHERE id=?")
            .bind(&id)
            .fetch_optional(auth.store.pool())
            .await
            .map_err(db)?;
    let snapshot = snapshot.ok_or_else(|| err(StatusCode::NOT_FOUND, "unknown_repository"))?;
    let item = parse_manifest(snapshot.as_bytes())?
        .into_iter()
        .find(|item| item.id == input.extension_id && item.version == input.version)
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "unknown_catalogue_version"))?;
    if matches!(item.id.as_str(), "avatars" | "poster-rotator") {
        return Err(err(StatusCode::BAD_REQUEST, "feature_is_native"));
    }
    if !item_compatible(&item) {
        return Err(err(StatusCode::BAD_REQUEST, "incompatible_server_version"));
    }
    let asset = fetch_asset(&item.source_url, MAX_DOWNLOAD).await?;
    if format!("{:x}", Sha256::digest(&asset)) != item.checksum {
        return Err(err(StatusCode::BAD_REQUEST, "invalid_package_digest"));
    }
    let package: Package = serde_json::from_slice(&package_bytes(&asset)?)
        .map_err(|_| err(StatusCode::BAD_REQUEST, "invalid_package"))?;
    // The signature (checked by the install paths) is the authority. The catalogue
    // entry must still describe this exact package, so an administrator never
    // installs something other than what was shown.
    let described = match &item.manifest {
        Some(manifest) => {
            serde_json::to_value(&package.manifest).ok() == serde_json::to_value(manifest).ok()
        }
        None => {
            package.manifest.id == item.id
                && package.manifest.version == item.version
                && (item.permissions.is_empty()
                    || same_set(&item.permissions, &package.manifest.permissions))
                && (item.http_hosts.is_empty()
                    || same_set(&item.http_hosts, &package.manifest.http_hosts))
        }
    };
    if package.publisher_id != item.publisher_id || !described {
        return Err(err(StatusCode::BAD_REQUEST, "catalogue_package_mismatch"));
    }
    if !compatible(&package.manifest) {
        return Err(err(StatusCode::BAD_REQUEST, "incompatible_server_version"));
    }
    let current: Option<String> =
        query_scalar("SELECT manifest_json FROM extension_repositories WHERE id=?")
            .bind(&id)
            .fetch_optional(auth.store.pool())
            .await
            .map_err(db)?;
    if current.as_deref() != Some(snapshot.as_str()) {
        return Err(err(StatusCode::CONFLICT, "repository_changed"));
    }
    if input.confirm_downgrade {
        if !input.upgrade {
            return Err(err(
                StatusCode::BAD_REQUEST,
                "extension_downgrade_confirmation_required",
            ));
        }
        let guard = super::DowngradeGuard {
            expected_version: input.expected_version.ok_or_else(|| {
                err(
                    StatusCode::BAD_REQUEST,
                    "extension_downgrade_confirmation_required",
                )
            })?,
            expected_sha256: input.expected_sha256.ok_or_else(|| {
                err(
                    StatusCode::BAD_REQUEST,
                    "extension_downgrade_confirmation_required",
                )
            })?,
        };
        super::change_version_with_key(
            auth,
            headers,
            input.extension_id,
            package,
            item.public_key,
            Some(guard),
        )
        .await
    } else if input.upgrade {
        super::upgrade_with_key(auth, headers, input.extension_id, package, item.public_key).await
    } else {
        super::install_with_key(auth, headers, package, item.public_key).await
    }
}

#[cfg(test)]
pub(super) fn test_documents()
-> &'static std::sync::Mutex<std::collections::HashMap<String, Vec<u8>>> {
    static DOCUMENTS: std::sync::LazyLock<
        std::sync::Mutex<std::collections::HashMap<String, Vec<u8>>>,
    > = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));
    &DOCUMENTS
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, header},
        response::Response,
    };
    use http_body_util::BodyExt;
    use ring::{
        rand::SystemRandom,
        signature::{Ed25519KeyPair, KeyPair},
    };
    use tower::ServiceExt;
    const ORIGIN: &str = "http://127.0.0.1:3000";
    async fn request(
        app: &Router,
        path: &str,
        method: &str,
        cookie: &str,
        csrf: &str,
        body: Value,
    ) -> Response {
        app.clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header("host", "127.0.0.1:3000")
                    .header("origin", ORIGIN)
                    .header(header::COOKIE, cookie)
                    .header("x-csrf-token", csrf)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap()
    }
    async fn body(response: Response) -> Value {
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap()
    }

    #[test]
    fn github_catalogue_links_keep_the_selected_branch() {
        for (page, endpoint) in [
            (
                "https://github.com/owner/plugins",
                "https://raw.githubusercontent.com/owner/plugins/main/repository/manifest.json",
            ),
            (
                "https://github.com/owner/plugins/tree/develop",
                "https://raw.githubusercontent.com/owner/plugins/develop/repository/manifest.json",
            ),
            (
                "https://github.com/owner/plugins/blob/develop/repository/manifest.json",
                "https://raw.githubusercontent.com/owner/plugins/develop/repository/manifest.json",
            ),
            (
                "https://raw.githubusercontent.com/owner/plugins/develop/repository/manifest.json",
                "https://raw.githubusercontent.com/owner/plugins/develop/repository/manifest.json",
            ),
            (
                "https://catalogue.example.com/custom.json",
                "https://catalogue.example.com/custom.json",
            ),
        ] {
            assert_eq!(catalogue_url(page).unwrap().as_str(), endpoint);
        }
        assert!(catalogue_url("http://github.com/owner/plugins/tree/develop").is_err());
        assert!(catalogue_url("https://user:secret@github.com/owner/plugins").is_err());
    }

    #[test]
    fn repository_url_and_address_policy_rejects_local_redirection_targets() {
        for url in [
            "http://example.com/manifest.json",
            "https://localhost/manifest.json",
            "https://127.0.0.1/x",
            "https://[::1]/x",
            "https://user:secret@example.com/x",
            "https://example.com:8443/x",
            "https://example.com/x#fragment",
        ] {
            assert!(safe_url(url).is_err(), "{url}");
        }
        for ip in [
            "127.0.0.1",
            "10.0.0.1",
            "169.254.169.254",
            "100.64.0.1",
            "198.18.0.1",
            "192.0.2.1",
            "::1",
            "::ffff:127.0.0.1",
            "fc00::1",
            "fe80::1",
            "2001:db8::1",
            "2002:7f00:1::1",
        ] {
            assert!(!public_ip(ip.parse().unwrap()), "{ip}");
        }
        assert!(public_ip("8.8.8.8".parse().unwrap()));
        assert!(public_ip("2001:4860:4860::8888".parse().unwrap()));
        assert!(safe_url("https://example.com/manifest.json").is_ok());
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "requires built Web assets and Chrome/Playwright; HTTPS documents injected only in this test build"]
    async fn browser_repository_catalogue_version_trust_and_install() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap();
        std::env::set_current_dir(&root).unwrap();
        let temp = tempfile::tempdir().unwrap();
        let media = temp.path().join("media");
        std::fs::create_dir(&media).unwrap();
        let store = crate::identity::IdentityStore::open(&temp.path().join("core.sqlite3"))
            .await
            .unwrap();
        store
            .bootstrap(
                "repo-browser",
                zeroize::Zeroizing::new("repository browser fixture password".into()),
            )
            .await
            .unwrap();
        let key_bytes = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
        let key = Ed25519KeyPair::from_pkcs8(key_bytes.as_ref()).unwrap();
        let public_key = STANDARD.encode(key.public_key().as_ref());
        let wasm=wat::parse_str("(module (memory (export \"memory\") 1 256) (func (export \"media_alloc\") (param i32) (result i32) i32.const 0) (func (export \"media_run\") (param i32 i32) (result i64) i64.const 0))").unwrap();
        let manifest_url = format!(
            "https://repository.example.com/{}/manifest.json",
            uuid::Uuid::new_v4()
        );
        let mut entries = Vec::new();
        let mut package_urls = Vec::new();
        for version in ["1.0.0", "1.1.0"] {
            let mut package = Package {
                manifest: Manifest {
                    api_version: 1,
                    id: "repo.browser".into(),
                    name: "Repository browser fixture".into(),
                    version: version.into(),
                    description: "Two signed versions for the repository browser flow".into(),
                    author: "Fixture author".into(),
                    license: "MIT".into(),
                    repository: "https://example.com/source".into(),
                    permissions: vec!["metadata.read".into()],
                    ..Default::default()
                },
                publisher_id: "repository-browser".into(),
                wasm: STANDARD.encode(&wasm),
                sha256: format!("{:x}", Sha256::digest(&wasm)),
                signature: String::new(),
            };
            package.signature =
                STANDARD.encode(key.sign(&package.signing_bytes().unwrap()).as_ref());
            let bytes = serde_json::to_vec(&package).unwrap();
            let package_url = format!(
                "https://repository.example.com/{}/plugin-{version}.json",
                uuid::Uuid::new_v4()
            );
            entries.push(json!({"manifest":package.manifest,"publisherId":package.publisher_id,"publicKey":public_key,"packageUrl":package_url,"packageSha256":format!("{:x}",Sha256::digest(&bytes))}));
            test_documents()
                .lock()
                .unwrap()
                .insert(package_url.clone(), bytes);
            package_urls.push(package_url);
        }
        test_documents().lock().unwrap().insert(
            manifest_url.clone(),
            serde_json::to_vec(&json!({"apiVersion":1,"extensions":entries})).unwrap(),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let origin = format!("http://127.0.0.1:{port}");
        let auth = AuthState::new(store.clone(), origin.clone()).await.unwrap();
        let app = crate::secure_app(
            crate::node::Inventory::local(media).await.unwrap(),
            port,
            auth,
        )
        .await
        .unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let result = tokio::process::Command::new("node")
            .arg("scripts/test-repositories-live.mjs")
            .env("MEDIA_TEST_ORIGIN", origin)
            .env("MEDIA_TEST_REPOSITORY_URL", &manifest_url)
            .current_dir(root.join("apps/web"))
            .status()
            .await
            .unwrap();
        server.abort();
        store.close().await;
        let mut documents = test_documents().lock().unwrap();
        documents.remove(&manifest_url);
        for url in package_urls {
            documents.remove(&url);
        }
        assert!(result.success(), "repository catalogue browser flow failed");
    }

    #[tokio::test]
    async fn default_repository_is_registered_once_and_deletion_survives_restart() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("default-repository.sqlite");
        let store = crate::identity::IdentityStore::open(&path).await.unwrap();
        let (url, snapshot): (String, String) = query_as(
            "SELECT url,manifest_json FROM extension_repositories WHERE id='community-default'",
        )
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(
            url,
            "https://raw.githubusercontent.com/maelmoreau21/Aether-Plugins/main/repository/manifest.json"
        );
        assert!(parse_manifest(snapshot.as_bytes()).unwrap().is_empty());
        let installed: i64 = query_scalar("SELECT COUNT(*) FROM extensions")
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert_eq!(installed, 0);
        query("DELETE FROM extension_repositories WHERE id='community-default'")
            .execute(store.pool())
            .await
            .unwrap();
        store.pool().close().await;
        let reopened = crate::identity::IdentityStore::open(&path).await.unwrap();
        let count: i64 = query_scalar(
            "SELECT COUNT(*) FROM extension_repositories WHERE id='community-default'",
        )
        .fetch_one(reopened.pool())
        .await
        .unwrap();
        assert_eq!(count, 0);
    }
    #[tokio::test]
    async fn repository_catalogue_install_preserves_trust_signature_and_persistence() {
        let temp = tempfile::tempdir().unwrap();
        let store = crate::identity::IdentityStore::open(&temp.path().join("core.sqlite3"))
            .await
            .unwrap();
        store
            .bootstrap(
                "repo-admin",
                zeroize::Zeroizing::new("repository fixture password long".into()),
            )
            .await
            .unwrap();
        let auth = AuthState::new(store.clone(), ORIGIN.into()).await.unwrap();
        let app = crate::auth::router(auth.clone()).merge(super::super::router(auth));
        let login = request(
            &app,
            "/api/v1/auth/login",
            "POST",
            "",
            "",
            json!({"login":"repo-admin","password":"repository fixture password long"}),
        )
        .await;
        assert_eq!(login.status(), StatusCode::OK);
        let cookie = login.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        let csrf = body(login).await["csrfToken"].as_str().unwrap().to_owned();
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
        let key = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
        let public = STANDARD.encode(key.public_key().as_ref());
        let wasm=wat::parse_str("(module (memory (export \"memory\") 1 256) (func (export \"media_alloc\") (param i32) (result i32) i32.const 0) (func (export \"media_run\") (param i32 i32) (result i64) i64.const 0))").unwrap();
        let mut package = Package {
            manifest: Manifest {
                api_version: 1,
                id: "repo.fixture".into(),
                name: "Repository fixture".into(),
                version: "1.0.0".into(),
                description: "Synthetic test".into(),
                author: "Tests".into(),
                license: "MIT".into(),
                repository: "https://example.com/source".into(),
                permissions: vec!["metadata.read".into()],
                ..Default::default()
            },
            publisher_id: "repository-test".into(),
            wasm: STANDARD.encode(&wasm),
            sha256: format!("{:x}", Sha256::digest(&wasm)),
            signature: String::new(),
        };
        package.signature = STANDARD.encode(key.sign(&package.signing_bytes().unwrap()).as_ref());
        let package_bytes = serde_json::to_vec(&package).unwrap();
        let package_url = format!(
            "https://packages.example.com/{}/plugin.json",
            uuid::Uuid::new_v4()
        );
        let manifest_url = format!(
            "https://packages.example.com/{}/manifest.json",
            uuid::Uuid::new_v4()
        );
        let manifest = json!({"apiVersion":1,"extensions":[{"manifest":package.manifest,"publisherId":package.publisher_id,"publicKey":public,"packageUrl":package_url,"packageSha256":format!("{:x}",Sha256::digest(&package_bytes))}]});
        test_documents()
            .lock()
            .unwrap()
            .insert(manifest_url.clone(), serde_json::to_vec(&manifest).unwrap());
        test_documents()
            .lock()
            .unwrap()
            .insert(package_url.clone(), package_bytes.clone());
        let input = json!({"name":"Repository test","url":manifest_url});
        assert_eq!(
            request(
                &app,
                "/api/v1/admin/extensions/repositories",
                "POST",
                "",
                "",
                input.clone()
            )
            .await
            .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            request(
                &app,
                "/api/v1/admin/extensions/repositories",
                "POST",
                &cookie,
                "",
                input.clone()
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
        let created = request(
            &app,
            "/api/v1/admin/extensions/repositories",
            "POST",
            &cookie,
            &csrf,
            input,
        )
        .await;
        assert_eq!(created.status(), StatusCode::CREATED);
        let id = body(created).await["id"].as_str().unwrap().to_owned();
        assert_eq!(
            body(
                request(
                    &app,
                    "/api/v1/admin/extensions/repositories",
                    "GET",
                    &cookie,
                    "",
                    json!(null)
                )
                .await
            )
            .await["repositories"]
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r["name"] == "Repository test")
                .unwrap()["name"],
            "Repository test"
        );
        let available = body(
            request(
                &app,
                "/api/v1/admin/extensions/catalogue",
                "GET",
                &cookie,
                "",
                json!(null),
            )
            .await,
        )
        .await;
        assert_eq!(available["entries"][0]["compatible"], true);
        let endpoint = format!("/api/v1/admin/extensions/repositories/{id}/install");
        let choice = json!({"extensionId":"repo.fixture","version":"1.0.0"});
        // A newly discovered publisher needs no approval request. Integrity is still checked.
        test_documents()
            .lock()
            .unwrap()
            .insert(package_url.clone(), b"tampered".to_vec());
        assert_eq!(
            request(&app, &endpoint, "POST", &cookie, &csrf, choice.clone())
                .await
                .status(),
            StatusCode::BAD_REQUEST
        );
        test_documents()
            .lock()
            .unwrap()
            .insert(package_url.clone(), package_bytes);
        assert_eq!(
            request(&app, &endpoint, "POST", &cookie, &csrf, choice)
                .await
                .status(),
            StatusCode::CREATED
        );
        let installed = body(
            request(
                &app,
                "/api/v1/admin/extensions",
                "GET",
                &cookie,
                "",
                json!(null),
            )
            .await,
        )
        .await;
        assert_eq!(installed["extensions"][0]["enabled"], false);
        assert_eq!(installed["extensions"][0]["libraryIds"], json!([]));
        // A catalogue update must use the guarded upgrade, never uninstall first.
        let before: (String, String) =
            query_as("SELECT package_json,generation FROM extensions WHERE id='repo.fixture'")
                .fetch_one(store.pool())
                .await
                .unwrap();
        let mut newer = package.clone();
        newer.manifest.version = "1.0.1".into();
        newer.signature = STANDARD.encode(key.sign(&newer.signing_bytes().unwrap()).as_ref());
        let newer_bytes = serde_json::to_vec(&newer).unwrap();
        let newer_manifest = json!({"apiVersion":1,"extensions":[{"manifest":newer.manifest,"publisherId":newer.publisher_id,"publicKey":public,"packageUrl":package_url,"packageSha256":format!("{:x}",Sha256::digest(&newer_bytes))}]});
        query("UPDATE extension_repositories SET manifest_json=? WHERE id=?")
            .bind(newer_manifest.to_string())
            .bind(&id)
            .execute(store.pool())
            .await
            .unwrap();
        test_documents()
            .lock()
            .unwrap()
            .insert(package_url.clone(), newer_bytes);
        let upgraded = request(
            &app,
            &endpoint,
            "POST",
            &cookie,
            &csrf,
            json!({"extensionId":"repo.fixture","version":"1.0.1","upgrade":true}),
        )
        .await;
        assert_eq!(upgraded.status(), StatusCode::CREATED);
        assert_eq!(body(upgraded).await["id"], "repo.fixture");
        let after: (String, String) =
            query_as("SELECT package_json,generation FROM extensions WHERE id='repo.fixture'")
                .fetch_one(store.pool())
                .await
                .unwrap();
        assert_ne!(before.0, after.0);
        assert_ne!(before.1, after.1);
        query("UPDATE extension_repositories SET manifest_json=? WHERE id=?")
            .bind(manifest.to_string())
            .bind(&id)
            .execute(store.pool())
            .await
            .unwrap();

        // Downgrade is explicit, guarded by the current revision and preserves a snapshot.
        test_documents()
            .lock()
            .unwrap()
            .insert(package_url.clone(), serde_json::to_vec(&package).unwrap());
        let downgrade = json!({"extensionId":"repo.fixture","version":"1.0.0","upgrade":true,"confirmDowngrade":true,"expectedVersion":"1.0.1","expectedSha256":newer.sha256});
        assert_eq!(
            request(
                &app,
                &endpoint,
                "POST",
                &cookie,
                &csrf,
                json!({"extensionId":"repo.fixture","version":"1.0.0","upgrade":true})
            )
            .await
            .status(),
            StatusCode::CONFLICT
        );
        let mut stale = downgrade.clone();
        stale["expectedVersion"] = json!("1.0.0");
        assert_eq!(
            request(&app, &endpoint, "POST", &cookie, &csrf, stale)
                .await
                .status(),
            StatusCode::CONFLICT
        );
        assert_eq!(
            request(&app, &endpoint, "POST", &cookie, "", downgrade.clone())
                .await
                .status(),
            StatusCode::FORBIDDEN
        );
        // A currently installed extension with private storage cannot be downgraded
        // without a verified inverse migration, even when its namespace is empty.
        let mut private_installed = newer.clone();
        private_installed
            .manifest
            .permissions
            .push("storage.private".into());
        query("UPDATE extensions SET package_json=? WHERE id='repo.fixture'")
            .bind(serde_json::to_string(&private_installed).unwrap())
            .execute(store.pool())
            .await
            .unwrap();
        let unsafe_attempt =
            request(&app, &endpoint, "POST", &cookie, &csrf, downgrade.clone()).await;
        assert_eq!(unsafe_attempt.status(), StatusCode::CONFLICT);
        assert_eq!(
            body(unsafe_attempt).await["error"],
            "extension_downgrade_incompatible"
        );
        query("UPDATE extensions SET package_json=? WHERE id='repo.fixture'")
            .bind(serde_json::to_string(&newer).unwrap())
            .execute(store.pool())
            .await
            .unwrap();
        // The 1.0.0 snapshot left by the upgrade holds no private data, so it is
        // replaced by a snapshot of 1.0.1 instead of blocking the downgrade.
        let snapshots: i64 = query_scalar(
            "SELECT COUNT(*) FROM extension_update_revisions WHERE extension_id='repo.fixture'",
        )
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(snapshots, 1);
        assert_eq!(
            request(&app, &endpoint, "POST", &cookie, &csrf, downgrade)
                .await
                .status(),
            StatusCode::CREATED
        );
        let restored = body(
            request(
                &app,
                "/api/v1/admin/extensions",
                "GET",
                &cookie,
                "",
                json!(null),
            )
            .await,
        )
        .await;
        assert_eq!(restored["extensions"][0]["manifest"]["version"], "1.0.0");
        assert_eq!(restored["extensions"][0]["enabled"], false);
        assert_eq!(restored["extensions"][0]["rollbackVersion"], "1.0.1");
        assert_eq!(request(&app, "/api/v1/admin/extensions/repo.fixture/rollback", "POST", &cookie, &csrf, json!({"expectedSha256":package.sha256,"expectedVersion":"1.0.0","confirmDataRestore":true})).await.status(), StatusCode::OK);

        assert_eq!(
            request(
                &app,
                &format!("/api/v1/admin/extensions/repositories/{id}/refresh"),
                "POST",
                &cookie,
                &csrf,
                json!(null)
            )
            .await
            .status(),
            StatusCode::NO_CONTENT
        );
        let count: i64 = query_scalar("SELECT COUNT(*) FROM extension_repositories WHERE id=?")
            .bind(&id)
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert_eq!(count, 1);
        assert_eq!(
            request(
                &app,
                &format!("/api/v1/admin/extensions/repositories/{id}"),
                "DELETE",
                &cookie,
                &csrf,
                json!(null)
            )
            .await
            .status(),
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            body(
                request(
                    &app,
                    "/api/v1/admin/extensions",
                    "GET",
                    &cookie,
                    "",
                    json!(null)
                )
                .await
            )
            .await["extensions"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        test_documents().lock().unwrap().remove(&manifest_url);
        test_documents().lock().unwrap().remove(&package_url);
    }

    fn zip(name: &str, content: &[u8], deflate: bool) -> Vec<u8> {
        use std::io::Write as _;
        let data = if deflate {
            let mut encoder =
                flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
            encoder.write_all(content).unwrap();
            encoder.finish().unwrap()
        } else {
            content.to_vec()
        };
        let mut crc = flate2::Crc::new();
        crc.update(content);
        let method: u16 = if deflate { 8 } else { 0 };
        let mut out = Vec::new();
        out.extend_from_slice(b"PK\x03\x04");
        for value in [20u16, 0, method, 0, 0] {
            out.extend_from_slice(&value.to_le_bytes());
        }
        for value in [crc.sum(), data.len() as u32, content.len() as u32] {
            out.extend_from_slice(&value.to_le_bytes());
        }
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&data);
        let directory = out.len();
        out.extend_from_slice(b"PK\x01\x02");
        for value in [20u16, 20, 0, method, 0, 0] {
            out.extend_from_slice(&value.to_le_bytes());
        }
        for value in [crc.sum(), data.len() as u32, content.len() as u32] {
            out.extend_from_slice(&value.to_le_bytes());
        }
        for value in [name.len() as u16, 0, 0, 0, 0] {
            out.extend_from_slice(&value.to_le_bytes());
        }
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        let size = out.len() - directory;
        out.extend_from_slice(b"PK\x05\x06");
        for value in [0u16, 0, 1, 1] {
            out.extend_from_slice(&value.to_le_bytes());
        }
        out.extend_from_slice(&(size as u32).to_le_bytes());
        out.extend_from_slice(&(directory as u32).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    #[test]
    fn release_assets_may_be_zips_with_bounded_extraction() {
        let package = br#"{"manifest":{}}"#;
        for deflate in [false, true] {
            let archive = zip("dist/plugin-1.0.0.package.json", package, deflate);
            assert_eq!(package_bytes(&archive).unwrap(), package);
        }
        assert_eq!(package_bytes(package).unwrap(), package);
        assert!(package_bytes(&zip("readme.txt", package, true)).is_err());
        let mut corrupted = zip("plugin.package.json", package, false);
        // Flip one byte of the stored data: the CRC-32 no longer matches.
        corrupted[30 + "plugin.package.json".len() + 2] ^= 1;
        assert!(package_bytes(&corrupted).is_err());
        // A highly compressible bomb is refused from its declared size.
        let bomb = zip("plugin.package.json", &vec![b' '; MAX_PACKAGE + 1], true);
        assert!(bomb.len() < 64 * 1024);
        assert!(package_bytes(&bomb).is_err());
        assert!(package_bytes(&vec![b' '; MAX_PACKAGE + 1]).is_err());
    }

    #[test]
    fn redirects_and_abi_follow_the_published_policy() {
        let url = |raw: &str| url::Url::parse(raw).unwrap();
        let release = url("https://github.com/o/r/releases/download/v1/p.zip");
        for allowed in [
            "https://objects.githubusercontent.com/x",
            "https://release-assets.githubusercontent.com/x?jwt=1",
            "https://codeload.github.com/x",
            "https://github.com/o/r/other",
        ] {
            assert!(redirect_allowed(&release, &url(allowed)), "{allowed}");
        }
        for refused in [
            "https://evil.example/x",
            "https://raw.githubusercontent.com/x",
        ] {
            assert!(!redirect_allowed(&release, &url(refused)), "{refused}");
        }
        assert!(!redirect_allowed(
            &url("https://example.com/manifest.json"),
            &url("https://objects.githubusercontent.com/x")
        ));
        assert!(abi_supported("1") && abi_supported("1.0") && abi_supported("1.0.0.0"));
        assert!(!abi_supported("1.1") && !abi_supported("2.0") && !abi_supported("0.9"));
        assert!(!abi_supported("1.0.0.0.0") && !abi_supported("x"));
    }

    #[test]
    fn versioned_catalogue_lists_every_version_and_keeps_limits() {
        let key = STANDARD.encode([7u8; 32]);
        let version = |number: &str| {
            json!({"version":number,"targetAbi":"1.0","sourceUrl":format!("https://github.com/o/r/releases/download/x-v{number}/x-{number}.zip"),
                "checksum":format!("sha256:{}", "A".repeat(64)),"timestamp":"2026-11-02T09:44:30Z","changelog":"- fix"})
        };
        let plugin = json!({"guid":"poster.example","name":"Example","description":"d","overview":"o","owner":"Mael","category":"Metadata",
            "imageUrl":"https://raw.githubusercontent.com/o/r/main/logo.png","publisherId":"mael","publicKey":key,
            "versions":[version("1.2.0"),version("1.1.0")]});
        let items = parse_manifest(&serde_json::to_vec(&json!([plugin.clone()])).unwrap()).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[1].version, "1.1.0");
        assert_eq!(items[0].checksum, "a".repeat(64));
        assert!(item_compatible(&items[0]));
        let object = json!({"apiVersion":2,"plugins":[plugin.clone()]});
        assert_eq!(
            parse_manifest(&serde_json::to_vec(&object).unwrap())
                .unwrap()
                .len(),
            2
        );
        let mut md5 = plugin.clone();
        md5["versions"][0]["checksum"] = json!("0".repeat(32));
        let mut duplicate = plugin.clone();
        duplicate["versions"][1]["version"] = json!("1.2.0");
        let mut redirect_url = plugin.clone();
        redirect_url["versions"][0]["sourceUrl"] = json!("http://github.com/x.zip");
        let mut algorithm = plugin.clone();
        algorithm["versions"][0]["checksumAlgorithm"] = json!("md5");
        for invalid in [md5, duplicate, redirect_url, algorithm] {
            assert!(parse_manifest(&serde_json::to_vec(&json!([invalid])).unwrap()).is_err());
        }
        let mut many = plugin;
        many["versions"] = json!(
            (0..=MAX_VERSIONS)
                .map(|n| version(&format!("1.0.{n}")))
                .collect::<Vec<_>>()
        );
        assert!(parse_manifest(&serde_json::to_vec(&json!([many])).unwrap()).is_err());
        let mut newer_abi = items[0].clone();
        newer_abi.target_abi = Some("1.1".into());
        assert!(!item_compatible(&newer_abi));
    }

    #[tokio::test]
    async fn versioned_catalogue_installs_zip_assets_and_older_versions() {
        let temp = tempfile::tempdir().unwrap();
        let store = crate::identity::IdentityStore::open(&temp.path().join("core.sqlite3"))
            .await
            .unwrap();
        store
            .bootstrap(
                "repo-admin",
                zeroize::Zeroizing::new("repository fixture password long".into()),
            )
            .await
            .unwrap();
        let auth = AuthState::new(store.clone(), ORIGIN.into()).await.unwrap();
        let app = crate::auth::router(auth.clone()).merge(super::super::router(auth));
        let login = request(
            &app,
            "/api/v1/auth/login",
            "POST",
            "",
            "",
            json!({"login":"repo-admin","password":"repository fixture password long"}),
        )
        .await;
        let cookie = login.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        let csrf = body(login).await["csrfToken"].as_str().unwrap().to_owned();
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
        let key = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
        let public = STANDARD.encode(key.public_key().as_ref());
        let wasm=wat::parse_str("(module (memory (export \"memory\") 1 256) (func (export \"media_alloc\") (param i32) (result i32) i32.const 0) (func (export \"media_run\") (param i32 i32) (result i64) i64.const 0))").unwrap();
        let run = uuid::Uuid::new_v4();
        let mut versions = Vec::new();
        let mut urls = Vec::new();
        for (number, settings) in [("1.0.0", 1), ("1.1.0", 2)] {
            let mut package = Package {
                manifest: Manifest {
                    api_version: 1,
                    id: "repo.versioned".into(),
                    name: "Versioned fixture".into(),
                    version: number.into(),
                    description: "Versioned catalogue".into(),
                    author: "Tests".into(),
                    license: "MIT".into(),
                    repository: "https://example.com/source".into(),
                    permissions: vec!["metadata.read".into()],
                    target_abi: Some("1.0".into()),
                    // The newer release adds an optional field: downgrading keeps saved values valid.
                    config_schema: Some(
                        (0..settings)
                            .map(|n| media_extension_sdk::ConfigField {
                                key: format!("field_{n}"),
                                label: "Field".into(),
                                field_type: "text".into(),
                                ..Default::default()
                            })
                            .collect(),
                    ),
                    ..Default::default()
                },
                publisher_id: "repository-test".into(),
                wasm: STANDARD.encode(&wasm),
                sha256: format!("{:x}", Sha256::digest(&wasm)),
                signature: String::new(),
            };
            package.signature =
                STANDARD.encode(key.sign(&package.signing_bytes().unwrap()).as_ref());
            let json_bytes = serde_json::to_vec(&package).unwrap();
            // The older release is published as a zip, the newer as the signed JSON.
            let (asset, url) = if number == "1.0.0" {
                (
                    zip("repo.versioned-1.0.0.package.json", &json_bytes, true),
                    format!("https://github.com/{run}/releases/download/v{number}/plugin.zip"),
                )
            } else {
                (
                    json_bytes,
                    format!(
                        "https://github.com/{run}/releases/download/v{number}/plugin.package.json"
                    ),
                )
            };
            versions.push(json!({"version":number,"targetAbi":"1.0","sourceUrl":url,"checksum":format!("{:x}",Sha256::digest(&asset)),
                "timestamp":format!("2026-11-0{}T09:44:30Z", settings),"changelog":format!("- release {number}"),"permissions":["metadata.read"]}));
            test_documents().lock().unwrap().insert(url.clone(), asset);
            urls.push(url);
        }
        versions.reverse();
        let image_url = format!("https://raw.githubusercontent.com/{run}/main/logo.svg");
        test_documents().lock().unwrap().insert(
            image_url.clone(),
            b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>".to_vec(),
        );
        let manifest_url = format!("https://raw.githubusercontent.com/{run}/main/manifest.json");
        let catalogue = json!({"apiVersion":2,"plugins":[{"id":"repo.versioned","name":"Versioned fixture","description":"Versioned catalogue",
            "owner":"Tests","category":"Metadata","imageUrl":image_url,"publisherId":"repository-test","publicKey":public,"versions":versions}]});
        test_documents().lock().unwrap().insert(
            manifest_url.clone(),
            serde_json::to_vec(&catalogue).unwrap(),
        );
        let created = request(
            &app,
            "/api/v1/admin/extensions/repositories",
            "POST",
            &cookie,
            &csrf,
            json!({"name":"Versioned","url":manifest_url}),
        )
        .await;
        assert_eq!(created.status(), StatusCode::CREATED);
        let id = body(created).await["id"].as_str().unwrap().to_owned();
        let listed = body(
            request(
                &app,
                "/api/v1/admin/extensions/catalogue",
                "GET",
                &cookie,
                "",
                json!(null),
            )
            .await,
        )
        .await;
        let entries: Vec<_> = listed["entries"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|entry| entry["repositoryId"] == id)
            .collect();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1]["manifest"]["version"], "1.0.0");
        assert_eq!(entries[1]["changelog"], "- release 1.0.0");
        assert_eq!(entries[1]["owner"], "Tests");
        assert_eq!(entries[0]["hasImage"], true);
        assert_eq!(entries[0]["compatible"], true);
        let image = body(
            request(
                &app,
                &format!("/api/v1/admin/extensions/repositories/{id}/image/repo.versioned"),
                "GET",
                &cookie,
                "",
                json!(null),
            )
            .await,
        )
        .await;
        assert!(
            image["dataUrl"]
                .as_str()
                .unwrap()
                .starts_with("data:image/svg+xml;base64,")
        );
        let endpoint = format!("/api/v1/admin/extensions/repositories/{id}/install");
        assert_eq!(
            request(
                &app,
                &endpoint,
                "POST",
                &cookie,
                &csrf,
                json!({"extensionId":"repo.versioned","version":"1.1.0"})
            )
            .await
            .status(),
            StatusCode::CREATED
        );
        let installed: String =
            query_scalar("SELECT package_json FROM extensions WHERE id='repo.versioned'")
                .fetch_one(store.pool())
                .await
                .unwrap();
        let installed: Package = serde_json::from_str(&installed).unwrap();
        // The previous version is installed from its zip through the confirmed downgrade path.
        let downgrade = json!({"extensionId":"repo.versioned","version":"1.0.0","upgrade":true,"confirmDowngrade":true,
            "expectedVersion":"1.1.0","expectedSha256":installed.sha256});
        let response = request(&app, &endpoint, "POST", &cookie, &csrf, downgrade).await;
        assert_eq!(response.status(), StatusCode::CREATED);
        let version: String =
            query_scalar("SELECT package_json FROM extensions WHERE id='repo.versioned'")
                .fetch_one(store.pool())
                .await
                .unwrap();
        assert_eq!(
            serde_json::from_str::<Package>(&version)
                .unwrap()
                .manifest
                .version,
            "1.0.0"
        );
        // A catalogue that misdescribes the signed permissions is refused.
        let mut misleading = catalogue.clone();
        misleading["plugins"][0]["versions"][0]["permissions"] = json!(["images"]);
        query("UPDATE extension_repositories SET manifest_json=? WHERE id=?")
            .bind(misleading.to_string())
            .bind(&id)
            .execute(store.pool())
            .await
            .unwrap();
        let refused = request(
            &app,
            &endpoint,
            "POST",
            &cookie,
            &csrf,
            json!({"extensionId":"repo.versioned","version":"1.1.0","upgrade":true}),
        )
        .await;
        assert_eq!(body(refused).await["error"], "catalogue_package_mismatch");
        let mut documents = test_documents().lock().unwrap();
        for url in urls.iter().chain([&manifest_url, &image_url]) {
            documents.remove(url);
        }
    }
}
