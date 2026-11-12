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

mod fetch;
mod handlers;
mod images;
mod manifest;
pub(crate) use fetch::*;
pub(in crate::extensions) use handlers::*;
use images::*;
pub(in crate::extensions) use manifest::*;

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
/// 1.1: requests up to `MAX_INPUT_JSON` (wider audio windows for marker detection).
const HOST_ABI: [u64; 4] = [1, 1, 0, 0];

/// Whether a package declares at least extension ABI 1.1, and so accepts requests larger than
/// `MAX_JSON`. A package without `targetAbi`, or with 1.0, gets requests of the 1.0 size.
pub(crate) fn accepts_large_input(target_abi: Option<&str>) -> bool {
    target_abi
        .and_then(manifest::abi)
        .is_some_and(|abi| abi >= [1, 1, 0, 0])
}

static DOWNLOADS: Semaphore = Semaphore::const_new(2);

static IMAGES: Semaphore = Semaphore::const_new(4);

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

#[cfg(test)]
mod tests;
