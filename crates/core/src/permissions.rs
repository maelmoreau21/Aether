//! Fail-closed permissions for the current synthetic library only.
//! No grant-management API or personal library inventory exists at this milestone.
use crate::{auth::AuthState, error, identity::Role};
use axum::{
    extract::{Request, State},
    http::{Method, StatusCode},
    middleware::Next,
    response::Response,
};

/// Administrators may inspect the fixture library. A normal account has no
/// implicit library rights. Explicit per-library grants must be implemented and
/// tested before allowing normal accounts or importing personal libraries.
pub fn can_read_fixture_library(role: Role) -> bool {
    matches!(role, Role::Administrator)
}

pub async fn require_media(
    State(auth): State<AuthState>,
    request: Request,
    next: Next,
) -> Response {
    let identity = match auth.authenticate(request.headers()).await {
        Ok(identity) => identity,
        Err(status) => return error(status, "authentication_required"),
    };
    if !can_read_fixture_library(identity.user.role) {
        return error(StatusCode::FORBIDDEN, "permission_denied");
    }
    // Read routes currently expose GET/HEAD only. Keep future mutations behind
    // the same CSRF guard rather than relying on browser UI visibility.
    if !matches!(
        request.method(),
        &Method::GET | &Method::HEAD | &Method::OPTIONS
    ) && let Err(status) = auth.require_csrf(request.headers(), &identity)
    {
        return error(status, "forbidden");
    }
    next.run(request).await
}
