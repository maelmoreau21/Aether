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

pub(crate) mod adapters;
mod admin;
mod artwork_rotation;
mod automated;
pub(crate) mod collections;
mod common;
pub mod data;
pub mod data_lifecycle;
pub mod files;
mod identify;
mod image_collection;
mod install;
mod matching;
pub mod objects;
mod package;
pub(crate) mod repositories;
mod router;
mod run;
mod run_ownership;
pub mod runtime;
mod settings;
pub mod storage;
pub(crate) mod subtitles;
#[cfg(test)]
mod tests;
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
