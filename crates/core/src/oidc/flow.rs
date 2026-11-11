//! Authorization-code flow: start, callback completion, cookies and logout URL.
use super::{
    config::endpoint,
    error::{Error, db, deny, invalid},
    types::{AppStart, AppToken, Callback, Flow, Oidc, SignIn, Tokens},
};
use crate::auth;
use axum::{
    Json,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{Html, IntoResponse, Redirect, Response},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};

impl Oidc {
    pub(super) async fn begin(
        &self,
        headers: &HeaderMap,
        link_session: Option<String>,
        app: Option<AppStart>,
    ) -> Result<Response, Error> {
        // The web interface starts from its own origin. A native application
        // opens the system browser instead: nothing but a one-time code bound
        // to its own verifier ever comes out of that flow.
        match &app {
            None => self
                .auth
                .check_origin(headers)
                .map_err(|s| deny(s, "forbidden"))?,
            Some(app) => {
                if !is_hex64(&app.challenge) || !is_app_redirect(&app.redirect) {
                    return Err(deny(StatusCode::BAD_REQUEST, "invalid_request"));
                }
            }
        }
        let _permit = self
            .workers
            .try_acquire()
            .map_err(|_| deny(StatusCode::TOO_MANY_REQUESTS, "try_later"))?;
        let c = self.config()?;
        let rescue: i64 = crate::database::query_scalar("SELECT CAST(EXISTS(SELECT 1 FROM users u JOIN local_credentials c ON c.user_id=u.id WHERE u.role='administrator' AND u.disabled_at IS NULL) AS INTEGER)")
            .fetch_one(&self.auth.store.pool).await.map_err(db)?;
        let has_any_admin: i64 = crate::database::query_scalar("SELECT CAST(EXISTS(SELECT 1 FROM users WHERE role='administrator' AND disabled_at IS NULL) AS INTEGER)")
            .fetch_one(&self.auth.store.pool).await.map_err(db)?;
        if rescue == 0 && has_any_admin == 0 && c.admin_groups.is_empty() {
            return Err(deny(StatusCode::FORBIDDEN, "local_administrator_required"));
        }
        let d = self.discover().await?;
        let state = auth::random_token();
        let browser = auth::random_token();
        let nonce = auth::random_token();
        let verifier = auth::random_token();
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        crate::database::query("DELETE FROM oidc_flows WHERE expires_at<=unixepoch()")
            .execute(&self.auth.store.pool)
            .await
            .map_err(db)?;
        let result = crate::database::query("INSERT INTO oidc_flows(state_hash,browser_hash,nonce,verifier,link_session_hash,expires_at) SELECT ?,?,?,?,?,unixepoch()+300 WHERE (SELECT count(*) FROM oidc_flows)<256")
            .bind(auth::hash(&state)).bind(auth::hash(&browser)).bind(&nonce).bind(&verifier).bind(link_session).execute(&self.auth.store.pool).await.map_err(db)?;
        if result.rows_affected() != 1 {
            return Err(deny(StatusCode::TOO_MANY_REQUESTS, "try_later"));
        }
        let mut url = endpoint(&d.authorization_endpoint)?;
        url.query_pairs_mut().extend_pairs([
            ("response_type", "code"),
            ("scope", c.scopes.as_str()),
            ("client_id", &c.client_id),
            ("redirect_uri", &c.redirect),
            ("state", &state),
            ("nonce", &nonce),
            ("code_challenge", &challenge),
            ("code_challenge_method", "S256"),
        ]);
        let mut response = match app {
            None => Json(serde_json::json!({"url":url.as_str()})).into_response(),
            Some(app) => {
                crate::database::query("DELETE FROM oidc_app_grants WHERE expires_at<=unixepoch()")
                    .execute(&self.auth.store.pool)
                    .await
                    .map_err(db)?;
                crate::database::query("INSERT INTO oidc_app_grants(state_hash,challenge,redirect,expires_at) VALUES(?,?,?,unixepoch()+300)")
                    .bind(auth::hash(&state)).bind(&app.challenge).bind(&app.redirect).execute(&self.auth.store.pool).await.map_err(db)?;
                Redirect::to(url.as_str()).into_response()
            }
        };
        response.headers_mut().insert(
            header::SET_COOKIE,
            HeaderValue::from_str(&self.flow_cookie(&browser, 300)).map_err(|_| invalid())?,
        );
        Ok(response)
    }

    pub(super) fn flow_cookie(&self, raw: &str, age: u32) -> String {
        format!(
            "aether_oidc_flow={raw}; Path=/api/v1/auth/oidc; HttpOnly; SameSite=Lax; Max-Age={age}{}",
            if self.auth.secure { "; Secure" } else { "" }
        )
    }

    pub(super) async fn finish(&self, headers: &HeaderMap, q: Callback) -> Result<Response, Error> {
        let _permit = self
            .workers
            .try_acquire()
            .map_err(|_| deny(StatusCode::TOO_MANY_REQUESTS, "try_later"))?;
        if q.state.len() != 64 {
            return Err(invalid());
        }
        let browser = flow_token(headers).ok_or_else(invalid)?;
        let state_hash = auth::hash(&q.state);
        // Atomic consume, bound to browser and deadline, even for provider errors.
        let flow: Flow = crate::database::query_as("DELETE FROM oidc_flows WHERE state_hash=? AND browser_hash=? AND expires_at>unixepoch() RETURNING browser_hash,nonce,verifier,link_session_hash")
            .bind(&state_hash).bind(auth::hash(browser)).fetch_optional(&self.auth.store.pool).await.map_err(db)?.ok_or_else(invalid)?;
        if flow.browser_hash != auth::hash(browser) {
            return Err(invalid());
        }
        let app: Option<String> = crate::database::query_scalar("SELECT redirect FROM oidc_app_grants WHERE state_hash=? AND code_hash IS NULL AND expires_at>unixepoch()")
            .bind(&state_hash).fetch_optional(&self.auth.store.pool).await.map_err(db)?;
        let outcome = self.sign_in(headers, q, flow, app.is_some()).await;
        let Some(redirect) = app else {
            let SignIn::Browser(raw) = outcome? else {
                return Err(invalid());
            };
            // End the cross-site redirect chain on a Core document. A same-origin
            // external script starts a new navigation so Strict session cookies and
            // the existing cross-site guards both remain effective on /app/.
            let mut response = Html(include_str!("complete.html")).into_response();
            response.headers_mut().append(
                header::SET_COOKIE,
                HeaderValue::from_str(&self.auth.cookie(&raw, auth::ABSOLUTE_SECONDS))
                    .map_err(|_| invalid())?,
            );
            return Ok(response);
        };
        // Application flow: the browser gets no session, only a one-time code
        // (or the reason of the refusal) to hand back to the application.
        match outcome {
            Ok(SignIn::App(user)) => {
                let code = auth::random_token();
                let issued = crate::database::query("UPDATE oidc_app_grants SET code_hash=?, user_id=?, expires_at=unixepoch()+120 WHERE state_hash=? AND code_hash IS NULL")
                    .bind(auth::hash(&code)).bind(&user).bind(&state_hash).execute(&self.auth.store.pool).await.map_err(db)?;
                if issued.rows_affected() != 1 {
                    return Err(invalid());
                }
                Ok(app_return(&redirect, "code", &code))
            }
            Ok(SignIn::Browser(_)) => Err(invalid()),
            Err((_, Json(body))) => {
                let _ = crate::database::query("DELETE FROM oidc_app_grants WHERE state_hash=?")
                    .bind(&state_hash)
                    .execute(&self.auth.store.pool)
                    .await;
                let code = body["error"]
                    .as_str()
                    .filter(|c| c.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'))
                    .unwrap_or("oidc_invalid_response");
                Ok(app_return(&redirect, "error", code))
            }
        }
    }

    /// Exchanges the provider's code, validates the identity and resolves the
    /// local user. The browser flow also opens its session here.
    async fn sign_in(
        &self,
        headers: &HeaderMap,
        q: Callback,
        flow: Flow,
        app: bool,
    ) -> Result<SignIn, Error> {
        let c = self.config()?;
        if q.error.is_some() {
            return Err(invalid());
        }
        let code = q
            .code
            .filter(|s| !s.is_empty() && s.len() < 4096)
            .ok_or_else(invalid)?;
        let d = self.discover().await?;

        // Client authentication negotiation at token endpoint:
        // If provider advertises token_endpoint_auth_methods_supported containing client_secret_post
        // and NOT client_secret_basic, use POST body authentication.
        // Otherwise, default to standard client_secret_basic (RFC 6749 2.3.1).
        let prefers_post = d
            .token_endpoint_auth_methods_supported
            .iter()
            .any(|m| m == "client_secret_post")
            && !d
                .token_endpoint_auth_methods_supported
                .iter()
                .any(|m| m == "client_secret_basic");

        let mut req = self.http.post(&d.token_endpoint);
        let mut form_params = vec![
            ("grant_type", "authorization_code".to_string()),
            ("code", code),
            ("redirect_uri", c.redirect.clone()),
            ("code_verifier", flow.verifier.clone()),
        ];

        if prefers_post {
            form_params.push(("client_id", c.client_id.clone()));
            form_params.push(("client_secret", c.secret.as_str().to_string()));
        } else {
            req = req.basic_auth(&c.client_id, Some(c.secret.as_str()));
        }

        let tokens: Tokens = self.json(req.form(&form_params)).await?;
        let claims = self.claims(&d, &tokens.id_token, &flow.nonce).await?;

        let extracted_groups = claims.extract_groups(&c.groups_claim);
        if c.require_group {
            let in_user_groups = extracted_groups
                .iter()
                .any(|g| c.groups.iter().any(|ug| ug.eq_ignore_ascii_case(g)));
            let in_admin_groups = !c.admin_groups.is_empty()
                && extracted_groups
                    .iter()
                    .any(|g| c.admin_groups.iter().any(|ag| ag.eq_ignore_ascii_case(g)));
            if !in_user_groups && !in_admin_groups {
                if let Ok(Some(existing_id)) = crate::database::query_scalar::<_, String>(
                    "SELECT user_id FROM oidc_identities WHERE issuer=? AND subject=?",
                )
                .bind(&c.issuer)
                .bind(&claims.sub)
                .fetch_optional(&self.auth.store.pool)
                .await
                {
                    let _ = crate::database::query("DELETE FROM sessions WHERE user_id=?")
                        .bind(&existing_id)
                        .execute(&self.auth.store.pool)
                        .await;
                }
                return Err(deny(StatusCode::FORBIDDEN, "oidc_group_denied"));
            }
        }
        let is_admin = !c.admin_groups.is_empty()
            && extracted_groups
                .iter()
                .any(|g| c.admin_groups.iter().any(|ag| ag.eq_ignore_ascii_case(g)));
        let role = if is_admin { "administrator" } else { "user" };
        let display_name = claims.display_name();
        let sso_username = claims.username();

        let mut conn = self.auth.store.pool.acquire().await.map_err(db)?;
        let mut tx = crate::database::begin_write(&mut conn).await.map_err(db)?;
        let linked_user: Option<String> = if let Some(ref session_hash) = flow.link_session_hash {
            Some(crate::database::query_scalar(
                "SELECT s.user_id FROM sessions s                  JOIN users u ON u.id=s.user_id                  JOIN local_credentials c ON c.user_id=u.id                  WHERE s.token_hash=? AND s.expires_at>unixepoch()                  AND s.last_seen_at>unixepoch()-? AND u.disabled_at IS NULL")
                .bind(session_hash).bind(crate::auth::IDLE_SECONDS).fetch_optional(&mut *tx).await.map_err(db)?
                .ok_or_else(|| deny(StatusCode::UNAUTHORIZED,"link_session_expired"))?)
        } else {
            None
        };
        let existing: Option<String> = crate::database::query_scalar(
            "SELECT user_id FROM oidc_identities WHERE issuer=? AND subject=?",
        )
        .bind(&c.issuer)
        .bind(&claims.sub)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db)?;
        let user = match (existing, linked_user) {
            (Some(id), Some(link)) if id != link => {
                return Err(deny(StatusCode::CONFLICT, "oidc_identity_already_linked"));
            }
            (Some(id), _) => {
                let has_local_cred: bool = crate::database::query_scalar::<_, i64>(
                    "SELECT CAST(EXISTS(SELECT 1 FROM local_credentials WHERE user_id=?) AS INTEGER)",
                )
                .bind(&id)
                .fetch_one(&mut *tx)
                .await
                .map_err(db)? != 0;

                let current_role: String =
                    crate::database::query_scalar("SELECT role FROM users WHERE id=?")
                        .bind(&id)
                        .fetch_one(&mut *tx)
                        .await
                        .map_err(db)?;

                if is_admin && !has_local_cred {
                    crate::database::query("UPDATE users SET role='administrator', display_name=COALESCE(?, display_name), sso_username=COALESCE(?, sso_username) WHERE id=?")
                        .bind(&display_name)
                        .bind(&sso_username)
                        .bind(&id)
                        .execute(&mut *tx)
                        .await
                        .map_err(db)?;
                } else if !has_local_cred
                    && !c.admin_groups.is_empty()
                    && current_role == "administrator"
                {
                    // Downgrade: SSO-managed user lost their admin group membership in IdP.
                    crate::database::query(
                        "UPDATE users SET role='user', display_name=COALESCE(?, display_name), sso_username=COALESCE(?, sso_username) WHERE id=?",
                    )
                    .bind(&display_name)
                    .bind(&sso_username)
                    .bind(&id)
                    .execute(&mut *tx)
                    .await
                    .map_err(db)?;

                    // Immediately revoke all existing sessions to invalidate stale administrator privileges.
                    crate::database::query("DELETE FROM sessions WHERE user_id=?")
                        .bind(&id)
                        .execute(&mut *tx)
                        .await
                        .map_err(db)?;
                } else {
                    crate::database::query(
                        "UPDATE users SET display_name=COALESCE(?, display_name), sso_username=COALESCE(?, sso_username) WHERE id=?",
                    )
                    .bind(&display_name)
                    .bind(&sso_username)
                    .bind(&id)
                    .execute(&mut *tx)
                    .await
                    .map_err(db)?;
                }
                id
            }
            (None, target) => {
                let id = target.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
                crate::database::query(
                    "INSERT INTO users(id,role,display_name,sso_username) VALUES (?,?,?,?) ON CONFLICT(id) DO UPDATE SET display_name=COALESCE(excluded.display_name, users.display_name), sso_username=COALESCE(excluded.sso_username, users.sso_username)",
                )
                .bind(&id)
                .bind(role)
                .bind(&display_name)
                .bind(&sso_username)
                .execute(&mut *tx)
                .await
                .map_err(db)?;
                let result = crate::database::query(
                    "INSERT INTO oidc_identities(issuer,subject,user_id) VALUES (?,?,?)",
                )
                .bind(&c.issuer)
                .bind(&claims.sub)
                .bind(&id)
                .execute(&mut *tx)
                .await;
                if result.is_err() {
                    return Err(deny(StatusCode::CONFLICT, "oidc_identity_already_linked"));
                }
                id
            }
        };
        if app {
            let active: i64 = crate::database::query_scalar("SELECT CAST(EXISTS(SELECT 1 FROM users WHERE id=? AND disabled_at IS NULL) AS INTEGER)")
                .bind(&user).fetch_one(&mut *tx).await.map_err(db)?;
            if active == 0 {
                return Err(deny(StatusCode::FORBIDDEN, "oidc_account_disabled"));
            }
            tx.commit().await.map_err(db)?;
            drop(conn);
            if crate::extensions::collections::assign_new_user(&self.auth, &user)
                .await
                .is_err()
            {
                eprintln!("Automatic avatar assignment failed");
            }
            return Ok(SignIn::App(user));
        }
        let raw = auth::random_token();
        let created=crate::database::query("INSERT INTO sessions(token_hash,user_id,created_at,last_seen_at,expires_at) SELECT ?,id,unixepoch(),unixepoch(),unixepoch()+2592000 FROM users WHERE id=? AND disabled_at IS NULL").bind(auth::hash(&raw)).bind(&user).execute(&mut *tx).await.map_err(db)?;
        crate::database::query("UPDATE sessions SET device_label=? WHERE token_hash=?")
            .bind(auth::device_label(headers))
            .bind(auth::hash(&raw))
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        if created.rows_affected() != 1 {
            return Err(deny(StatusCode::FORBIDDEN, "oidc_account_disabled"));
        }
        // Rotate the original local session after linking; local credential is preserved.
        if let Some(old) = flow.link_session_hash {
            crate::database::query("DELETE FROM sessions WHERE token_hash=?")
                .bind(old)
                .execute(&mut *tx)
                .await
                .map_err(db)?;
        }
        tx.commit().await.map_err(db)?;
        drop(conn);
        if crate::extensions::collections::assign_new_user(&self.auth, &user)
            .await
            .is_err()
        {
            eprintln!("Automatic avatar assignment failed");
        }
        Ok(SignIn::Browser(raw))
    }

    /// The application redeems its one-time code with the verifier only it
    /// knows, for a device token (or a cookie, for an application that shows
    /// the web interface).
    pub(super) async fn redeem(
        &self,
        headers: &HeaderMap,
        input: AppToken,
    ) -> Result<Response, Error> {
        let bearer = input.token_type == "bearer";
        if bearer {
            self.auth.check_origin_or_native(headers)
        } else {
            self.auth.check_origin(headers)
        }
        .map_err(|s| deny(s, "forbidden"))?;
        if !is_hex64(&input.code) || !is_hex64(&input.verifier) {
            return Err(deny(StatusCode::BAD_REQUEST, "invalid_request"));
        }
        // Single use: a wrong verifier burns the code.
        let grant: Option<(String, Option<String>)> = crate::database::query_as("DELETE FROM oidc_app_grants WHERE code_hash=? AND expires_at>unixepoch() RETURNING challenge,user_id")
            .bind(auth::hash(&input.code)).fetch_optional(&self.auth.store.pool).await.map_err(db)?;
        let Some((challenge, Some(user))) = grant else {
            return Err(invalid());
        };
        if auth::hash(&input.verifier) != challenge {
            return Err(invalid());
        }
        Ok(auth::issue_session(&self.auth, headers, &user, bearer).await)
    }

    pub(super) async fn logout_url(&self) -> Result<Option<String>, Error> {
        let d = self.discover().await?;
        let Some(ese) = d.end_session_endpoint else {
            return Ok(None);
        };
        let mut url = endpoint(&ese)?;
        url.query_pairs_mut()
            .append_pair("post_logout_redirect_uri", self.auth.origin());
        Ok(Some(url.to_string()))
    }
}

fn is_hex64(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Only the application itself can be handed the code: its own scheme on a
/// phone, a loopback port it listens on for a desktop application.
fn is_app_redirect(redirect: &str) -> bool {
    if redirect == "aether://sso" {
        return true;
    }
    redirect
        .strip_prefix("http://127.0.0.1:")
        .and_then(|rest| rest.strip_suffix("/sso"))
        .is_some_and(|port| {
            !port.starts_with('0')
                && port.bytes().all(|b| b.is_ascii_digit())
                && port.parse::<u16>().is_ok_and(|p| p >= 1024)
        })
}

/// Sends the browser back to the application. A loopback address is a plain
/// redirect; the application scheme goes through a page, because browsers
/// refuse to leave for another application without a visible link to fall
/// back on.
fn app_return(redirect: &str, key: &str, value: &str) -> Response {
    let url = format!("{redirect}?{key}={value}");
    if redirect.starts_with("http://") {
        return Redirect::to(&url).into_response();
    }
    let title = if key == "code" {
        "Connexion réussie"
    } else {
        "Connexion refusée"
    };
    Html(
        include_str!("app_return.html")
            .replace("{title}", title)
            .replace("{url}", &url),
    )
    .into_response()
}

fn flow_token(headers: &HeaderMap) -> Option<&str> {
    let mut found = None;
    for h in headers.get_all(header::COOKIE) {
        for pair in h.to_str().ok()?.split(';') {
            if let Some(("aether_oidc_flow", v)) = pair.trim().split_once('=') {
                if found.is_some() || v.len() != 64 || !v.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return None;
                }
                found = Some(v);
            }
        }
    }
    found
}
