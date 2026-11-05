//! Environment-driven OIDC configuration and issuer/endpoint validation.
use super::error::{Error, invalid};
use crate::auth::AuthState;
use std::sync::Arc;
use url::Url;
use zeroize::Zeroizing;

#[derive(Clone)]
pub(super) struct Config {
    pub(super) issuer: String,
    pub(super) client_id: String,
    pub(super) secret: Arc<Zeroizing<String>>,
    pub(super) redirect: String,
    pub(super) scopes: String,
    pub(super) groups: Vec<String>,
    pub(super) admin_groups: Vec<String>,
    pub(super) groups_claim: String,
    pub(super) require_group: bool,
}
impl Config {
    pub(super) fn from_env(auth: &AuthState) -> Result<Option<Self>, &'static str> {
        let enabled = std::env::var("MEDIA_OIDC_ENABLED").unwrap_or_else(|_| "false".into());
        if enabled == "false" {
            return Ok(None);
        }
        if enabled != "true" {
            return Err("MEDIA_OIDC_ENABLED must be true or false");
        }
        let required = |name| {
            std::env::var(name)
                .ok()
                .filter(|s| !s.trim().is_empty())
                .ok_or("missing required MEDIA_OIDC configuration")
        };
        let issuer = required("MEDIA_OIDC_ISSUER")?;
        let url = endpoint(&issuer).map_err(|_| "OIDC issuer must be HTTPS (HTTP loopback only), without credentials, query or fragment")?;
        if url.query().is_some() {
            return Err("OIDC issuer cannot have a query");
        }

        let scopes = std::env::var("MEDIA_OIDC_SCOPES")
            .ok()
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "openid profile email".to_string());

        let groups_claim = std::env::var("MEDIA_OIDC_GROUPS_CLAIM")
            .ok()
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "groups".to_string());

        let require_group = std::env::var("MEDIA_OIDC_REQUIRE_GROUP")
            .map(|v| v.trim().eq_ignore_ascii_case("true"))
            .unwrap_or(true);

        let groups: Vec<String> = std::env::var("MEDIA_OIDC_USER_GROUPS")
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();

        let admin_groups: Vec<String> = std::env::var("MEDIA_OIDC_ADMIN_GROUPS")
            .or_else(|_| std::env::var("MEDIA_OIDC_ADMIN_GROUP"))
            .or_else(|_| std::env::var("OIDC_ADMIN_GROUP"))
            .or_else(|_| std::env::var("OIDC_ADMIN_GROUPS"))
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();

        if require_group && groups.is_empty() && admin_groups.is_empty() {
            return Err(
                "MEDIA_OIDC_USER_GROUPS or MEDIA_OIDC_ADMIN_GROUPS must explicitly allow at least one group when OIDC is enabled. If you intentionally want to allow all users from this IdP without group filtering, explicitly set MEDIA_OIDC_REQUIRE_GROUP=false",
            );
        }

        Ok(Some(Self {
            issuer,
            client_id: required("MEDIA_OIDC_CLIENT_ID")?,
            secret: Arc::new(Zeroizing::new(required("MEDIA_OIDC_CLIENT_SECRET")?)),
            redirect: format!("{}/api/v1/auth/oidc/callback", auth.origin()),
            scopes,
            groups,
            admin_groups,
            groups_claim,
            require_group,
        }))
    }
}

pub(super) fn endpoint(raw: &str) -> Result<Url, Error> {
    let u = Url::parse(raw).map_err(|_| invalid())?;
    if !u.username().is_empty()
        || u.password().is_some()
        || u.fragment().is_some()
        || !(u.scheme() == "https"
            || (u.scheme() == "http"
                && matches!(u.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))))
    {
        return Err(invalid());
    }
    Ok(u)
}

pub(super) fn issuers_match(a: &str, b: &str) -> bool {
    a.trim_end_matches('/') == b.trim_end_matches('/')
}
