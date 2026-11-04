use crate::{
    database::query_scalar,
    identity::{IdentityStore, Role},
    product::parental_allowed,
};

// ---------------------------------------------------------------------------
// Authorization helper: Verifies library access and parental controls.
// ---------------------------------------------------------------------------

pub async fn can_user_access_media(
    store: &IdentityStore,
    user_id: &str,
    user_role: &Role,
    media_id: &str,
) -> bool {
    let raw_id = media_id.trim();
    if raw_id.is_empty() {
        return false;
    }

    // Resolve if raw_id is an episode ID
    let resolved_work_id: String =
        query_scalar("SELECT COALESCE((SELECT work_id FROM inventory_episodes WHERE id = ?), ?)")
            .bind(raw_id)
            .bind(raw_id)
            .fetch_optional(&store.pool)
            .await
            .unwrap_or(None)
            .unwrap_or_else(|| raw_id.to_string());

    // 1. Check parental policy on resolved work
    let parental_ok = parental_allowed(store, user_id, &resolved_work_id)
        .await
        .unwrap_or(false);
    if !parental_ok {
        return false;
    }

    // 2. Check library permission on resolved work
    let allowed: Option<i64> = query_scalar(
        "SELECT COUNT(*) FROM inventory_works w \
         JOIN libraries l ON l.id = w.library_id \
         JOIN media_nodes n ON n.id = l.node_id \
         WHERE w.id = ? AND l.enabled = 1 AND n.status != 'revoked' \
         AND (? = 'administrator' OR EXISTS( \
             SELECT 1 FROM effective_library_permissions p \
             WHERE p.library_id = l.id AND p.user_id = ? \
         ))",
    )
    .bind(&resolved_work_id)
    .bind(user_role.as_str())
    .bind(user_id)
    .fetch_optional(&store.pool)
    .await
    .unwrap_or(None);

    allowed.unwrap_or(0) > 0
}
