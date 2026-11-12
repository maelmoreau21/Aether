//! OIDC REST router and handlers.
use super::{
    config::Config,
    error::{Error, db, deny, invalid},
    types::{AppStart, AppToken, Callback, Link, Oidc, Status},
};
use crate::{
    auth::{self, AuthState},
    identity::Role,
};
use axum::{
    Json, Router,
    extract::{Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use std::{sync::Arc, time::Duration};
use zeroize::Zeroizing;

pub fn router(auth: AuthState) -> Result<Router, Box<dyn std::error::Error + Send + Sync>> {
    let config = Config::from_env(&auth).map_err(std::io::Error::other)?;
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .connect_timeout(Duration::from_secs(4))
        .build()?;
    Ok(routes(Oidc {
        auth,
        config,
        http,
        workers: Arc::new(tokio::sync::Semaphore::new(8)),
    }))
}

pub(super) fn routes(state: Oidc) -> Router {
    Router::new()
        .route("/api/v1/auth/oidc/status", get(public_status))
        .route(
            "/api/v1/auth/oidc/complete.js",
            get(|| async {
                (
                    [(
                        header::CONTENT_TYPE,
                        "application/javascript; charset=utf-8",
                    )],
                    "window.location.replace('/app/');",
                )
            }),
        )
        .route("/api/v1/admin/oidc", get(admin_status))
        .route("/api/v1/auth/oidc/start", post(start))
        .route("/api/v1/auth/oidc/link", post(link))
        .route("/api/v1/auth/oidc/app/start", get(app_start))
        .route("/api/v1/auth/oidc/app/token", post(app_token))
        .route(
            "/api/v1/auth/oidc/app/return.js",
            get(|| async {
                (
                    [(
                        header::CONTENT_TYPE,
                        "application/javascript; charset=utf-8",
                    )],
                    "window.location.replace(document.getElementById('open').href);",
                )
            }),
        )
        .route("/api/v1/auth/oidc/callback", get(callback))
        .route("/api/v1/auth/oidc/logout", get(logout))
        .layer(axum::extract::DefaultBodyLimit::max(4096))
        .with_state(state)
}

async fn public_status(State(s): State<Oidc>) -> Result<Json<serde_json::Value>, Error> {
    let button_label = crate::server_settings::load(&s.auth.store)
        .await
        .map_err(|status| deny(status, "internal_error"))?
        .sso_button_label;
    let local_login_enabled = crate::auth::is_local_login_enabled();
    if let Some(c) = &s.config {
        let issuer_lower = c.issuer.to_lowercase();
        let provider = if issuer_lower.contains("authentik") {
            "Authentik"
        } else if issuer_lower.contains("keycloak") {
            "Keycloak"
        } else if issuer_lower.contains("dex") {
            "Dex"
        } else if issuer_lower.contains("pocket-id") || issuer_lower.contains("pocketid") {
            "Pocket ID"
        } else {
            "SSO"
        };
        Ok(Json(serde_json::json!({
            "enabled": true,
            "provider": provider,
            "localLoginEnabled": local_login_enabled,
            "buttonLabel": button_label
        })))
    } else {
        Ok(Json(serde_json::json!({
            "enabled": false,
            "localLoginEnabled": local_login_enabled,
            "buttonLabel": button_label
        })))
    }
}

async fn admin_status(State(s): State<Oidc>, headers: HeaderMap) -> Result<Json<Status>, Error> {
    let identity = s
        .auth
        .authenticate(&headers)
        .await
        .map_err(|st| deny(st, "authentication_required"))?;
    if identity.user.role != Role::Administrator {
        return Err(deny(StatusCode::FORBIDDEN, "forbidden"));
    }

    let end_session_endpoint = if s.config.is_some() {
        s.discover().await.ok().and_then(|d| d.end_session_endpoint)
    } else {
        None
    };

    Ok(Json(Status {
        enabled: s.config.is_some(),
        issuer: s.config.as_ref().map(|c| c.issuer.clone()),
        client_id: s.config.as_ref().map(|c| c.client_id.clone()),
        redirect_uri: s.config.as_ref().map(|c| c.redirect.clone()),
        scopes: s.config.as_ref().map(|c| c.scopes.clone()),
        groups_claim: s.config.as_ref().map(|c| c.groups_claim.clone()),
        require_group: s.config.as_ref().is_some_and(|c| c.require_group),
        user_groups: s
            .config
            .as_ref()
            .map(|c| c.groups.clone())
            .unwrap_or_default(),
        admin_groups: s
            .config
            .as_ref()
            .map(|c| c.admin_groups.clone())
            .unwrap_or_default(),
        end_session_endpoint,
        local_login_enabled: crate::auth::is_local_login_enabled(),
    }))
}

async fn logout(State(s): State<Oidc>) -> Result<Json<serde_json::Value>, Error> {
    let url = s.logout_url().await?;
    Ok(Json(serde_json::json!({ "endSessionUrl": url })))
}
async fn start(State(s): State<Oidc>, headers: HeaderMap) -> Result<Response, Error> {
    s.begin(&headers, None, None).await
}
/// Opened by a native application in the system browser: straight to the
/// identity provider, and back to the application with a one-time code.
async fn app_start(
    State(s): State<Oidc>,
    headers: HeaderMap,
    Query(app): Query<AppStart>,
) -> Result<Response, Error> {
    let mut response = s.begin(&headers, None, Some(app)).await?;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}
async fn app_token(
    State(s): State<Oidc>,
    headers: HeaderMap,
    Json(input): Json<AppToken>,
) -> Result<Response, Error> {
    s.redeem(&headers, input).await
}
async fn link(
    State(s): State<Oidc>,
    headers: HeaderMap,
    Json(input): Json<Link>,
) -> Result<Response, Error> {
    use argon2::{
        Argon2,
        password_hash::{PasswordVerifier, phc::PasswordHash},
    };
    let identity = s
        .auth
        .authenticate(&headers)
        .await
        .map_err(|st| deny(st, "authentication_required"))?;
    s.auth
        .require_csrf(&headers, &identity)
        .map_err(|st| deny(st, "forbidden"))?;
    if input.password.len() > 512 {
        return Err(invalid());
    }
    // Reuse local login's persistent client rate limit before expensive Argon2 work.
    let client_key = auth::extract_client_key(&headers);
    let allowed = crate::database::query_scalar::<_, i64>(
        "INSERT INTO login_throttle(throttle_key,window_started,attempts) \
                VALUES(?,unixepoch(),1) ON CONFLICT(throttle_key) DO UPDATE \
                SET attempts=CASE WHEN login_throttle.window_started<=unixepoch()-60 THEN 1 \
                ELSE login_throttle.attempts+1 END, window_started=CASE \
                WHEN login_throttle.window_started<=unixepoch()-60 THEN unixepoch() \
                ELSE login_throttle.window_started END \
                RETURNING login_throttle.attempts",
    )
    .bind(&client_key)
    .fetch_one(&s.auth.store.pool)
    .await
    .map_err(db)?;
    if allowed > 10 {
        return Err(deny(StatusCode::TOO_MANY_REQUESTS, "try_later"));
    }
    let permit = s
        .auth
        .workers
        .clone()
        .try_acquire_owned()
        .map_err(|_| deny(StatusCode::TOO_MANY_REQUESTS, "try_later"))?;
    let encoded: String = crate::database::query_scalar(
        "SELECT password_hash FROM local_credentials WHERE user_id=?",
    )
    .bind(&identity.user.id)
    .fetch_optional(&s.auth.store.pool)
    .await
    .map_err(db)?
    .ok_or_else(invalid)?;
    let password = Zeroizing::new(input.password);
    let ok = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        PasswordHash::new(&encoded).is_ok_and(|h| {
            Argon2::default()
                .verify_password(password.as_bytes(), &h)
                .is_ok()
        })
    })
    .await
    .unwrap_or(false);
    if !ok {
        return Err(invalid());
    }
    s.begin(&headers, Some(identity.token_hash), None).await
}
async fn callback(
    State(s): State<Oidc>,
    headers: HeaderMap,
    Query(q): Query<Callback>,
) -> Response {
    let mut response = match s.finish(&headers, q).await {
        Ok(r) => r,
        Err(e) => e.into_response(),
    };
    if let Ok(v) = HeaderValue::from_str(&s.flow_cookie("", 0)) {
        response.headers_mut().append(header::SET_COOKIE, v);
    }
    response.headers_mut().insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}
