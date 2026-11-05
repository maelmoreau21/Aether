//! Shared OIDC state, provider payloads and request/response shapes.
use super::config::Config;
use crate::auth::AuthState;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone)]
pub(super) struct Oidc {
    pub(super) auth: AuthState,
    pub(super) config: Option<Config>,
    pub(super) http: reqwest::Client,
    pub(super) workers: Arc<tokio::sync::Semaphore>,
}

#[derive(Deserialize)]
pub(super) struct Discovery {
    pub(super) issuer: String,
    pub(super) authorization_endpoint: String,
    pub(super) token_endpoint: String,
    pub(super) jwks_uri: String,
    #[serde(default)]
    pub(super) end_session_endpoint: Option<String>,
    pub(super) id_token_signing_alg_values_supported: Vec<String>,
    #[serde(default)]
    pub(super) token_endpoint_auth_methods_supported: Vec<String>,
}

#[derive(Deserialize)]
pub(super) struct Tokens {
    pub(super) id_token: String,
}

#[derive(Deserialize)]
pub(super) struct Callback {
    pub(super) state: String,
    #[serde(default)]
    pub(super) code: Option<String>,
    #[serde(default)]
    pub(super) error: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct Link {
    pub(super) password: String,
}

#[derive(sqlx::FromRow)]
pub(super) struct Flow {
    pub(super) browser_hash: String,
    pub(super) nonce: String,
    pub(super) verifier: String,
    pub(super) link_session_hash: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Status {
    pub(super) enabled: bool,
    pub(super) issuer: Option<String>,
    pub(super) client_id: Option<String>,
    pub(super) redirect_uri: Option<String>,
    pub(super) scopes: Option<String>,
    pub(super) groups_claim: Option<String>,
    pub(super) require_group: bool,
    pub(super) user_groups: Vec<String>,
    pub(super) admin_groups: Vec<String>,
    pub(super) end_session_endpoint: Option<String>,
    pub(super) local_login_enabled: bool,
}
