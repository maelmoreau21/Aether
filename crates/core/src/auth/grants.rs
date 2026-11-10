//! Playback grants: capability URLs for players that cannot send a cookie or
//! an `Authorization` header (Chromecast, AirPlay, an HLS player following
//! relative segment URIs). Random, stored as SHA-256, bound to one asset and to
//! the issuing session: a logout or a revocation ends them, and every request
//! revalidates that session and the account's live restrictions.
use super::{
    AuthState, Authenticated,
    util::{hash, random_token},
};
use axum::http::StatusCode;

/// Long enough for a film with pauses, short enough for a URL that may be logged.
pub(crate) const GRANT_SECONDS: i64 = 12 * 60 * 60;
const MAX_GRANTS_PER_SESSION: i64 = 32;

fn internal(_: sqlx::Error) -> StatusCode {
    StatusCode::INTERNAL_SERVER_ERROR
}

impl AuthState {
    pub(crate) async fn issue_playback_grant(
        &self,
        identity: &Authenticated,
        asset_id: &str,
    ) -> Result<(String, i64), StatusCode> {
        crate::database::query("DELETE FROM playback_grants WHERE expires_at<=unixepoch()")
            .execute(&self.store.pool)
            .await
            .map_err(internal)?;
        let raw = random_token();
        let created: Option<i64> = crate::database::query_scalar("INSERT INTO playback_grants(token_hash,session_hash,asset_id,expires_at) SELECT ?,?,?,unixepoch()+? WHERE (SELECT COUNT(*) FROM playback_grants WHERE session_hash=?)<? RETURNING expires_at")
            .bind(hash(&raw))
            .bind(&identity.token_hash)
            .bind(asset_id)
            .bind(GRANT_SECONDS)
            .bind(&identity.token_hash)
            .bind(MAX_GRANTS_PER_SESSION)
            .fetch_optional(&self.store.pool)
            .await
            .map_err(internal)?;
        created
            .map(|expires| (raw, expires))
            .ok_or(StatusCode::TOO_MANY_REQUESTS)
    }

    /// The identity behind a grant, for this asset only. It behaves like a bearer
    /// request: no cookie is involved, so CSRF does not apply.
    pub(crate) async fn authenticate_grant(
        &self,
        grant: &str,
        asset_id: &str,
    ) -> Result<Authenticated, StatusCode> {
        if grant.len() != 64 || !grant.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(StatusCode::UNAUTHORIZED);
        }
        let session: Option<String> = crate::database::query_scalar(
            "SELECT session_hash FROM playback_grants WHERE token_hash=? AND asset_id=? AND expires_at>unixepoch()",
        )
        .bind(hash(grant))
        .bind(asset_id)
        .fetch_optional(&self.store.pool)
        .await
        .map_err(internal)?;
        let token_hash = session.ok_or(StatusCode::UNAUTHORIZED)?;
        let user = self.session_from_hash(&token_hash).await?;
        Ok(Authenticated {
            user,
            csrf_token: String::new(),
            token_hash,
            bearer: true,
        })
    }
}
