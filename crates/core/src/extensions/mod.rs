//! Extension v1: signed local packages, explicit grants and disposable Wasm workers.
//!
//! Module layout:
//! - `common`: shared error type and small helpers.
//! - `package`: signed package validation.
//! - `router`: HTTP routes and initialisation.
//! - `admin`: publishers, audit and installed-extension listing.
//! - `install`: install, upgrade, bootstrap and uninstall flows.
//! - `settings`: effective settings and configuration endpoints.
//! - `run`: manual runs, worker invocation and run history.
//! - `automated`: automated (scan/refresh) provider runs.
//! - `matching`: trusted candidate matching for metadata and music.
use crate::{
    auth::AuthState,
    database::{query, query_as, query_scalar},
};
#[cfg(test)]
use axum::http::HeaderValue;
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    routing::get,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use media_extension_sdk::{ExtensionRequest, Manifest, MetadataProposal, Package};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{process::Stdio, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::{Mutex, Semaphore},
};

static MUTATIONS: Mutex<()> = Mutex::const_new(());

#[path = "../extension_adapters.rs"]
pub(crate) mod adapters;
mod admin;
#[path = "../extension_artwork_rotation.rs"]
mod artwork_rotation;
mod automated;
#[path = "../extension_avatar_collections.rs"]
pub(crate) mod collections;
mod common;
#[path = "../extension_identify.rs"]
mod identify;
#[path = "../extension_image_collection.rs"]
mod image_collection;
mod install;
mod matching;
mod package;
#[path = "../extension_repositories.rs"]
pub(crate) mod repositories;
mod router;
mod run;
#[path = "../extension_run_ownership.rs"]
mod run_ownership;
mod settings;
#[path = "../extension_subtitles.rs"]
pub(crate) mod subtitles;
#[cfg(test)]
mod tests;
#[path = "../extension_updates.rs"]
mod updates;

use admin::*;
use common::*;
use install::*;
use matching::*;
use package::*;
use run::*;
use settings::*;

pub(crate) use automated::run_automated;
pub use install::{bootstrap_essential_plugins, install_internal_with_key};
pub use matching::trusted_music_candidate;
pub use package::validate_package;
pub use router::{initialize, router};
