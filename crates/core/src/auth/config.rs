//! Environment-driven authentication configuration.

pub fn is_local_login_enabled() -> bool {
    let disabled = std::env::var("MEDIA_AUTH_LOCAL_ENABLED")
        .map(|v| v.trim().eq_ignore_ascii_case("false"))
        .unwrap_or(false)
        || std::env::var("MEDIA_AUTH_DISABLE_LOCAL_LOGIN")
            .map(|v| v.trim().eq_ignore_ascii_case("true"))
            .unwrap_or(false)
        || std::env::var("MEDIA_OIDC_ONLY")
            .map(|v| v.trim().eq_ignore_ascii_case("true"))
            .unwrap_or(false);
    !disabled
}

pub fn is_http_bootstrap_disabled() -> bool {
    std::env::var("MEDIA_DISABLE_HTTP_BOOTSTRAP")
        .map(|v| v.trim().eq_ignore_ascii_case("true"))
        .unwrap_or(false)
        || std::env::var("MEDIA_BOOTSTRAP_HTTP_ENABLED")
            .map(|v| v.trim().eq_ignore_ascii_case("false"))
            .unwrap_or(false)
}

pub(super) fn resolve_allowed_origins(primary_origin: &str) -> Vec<String> {
    let mut origins = Vec::new();

    let env_val =
        std::env::var("MEDIA_ALLOWED_ORIGINS").or_else(|_| std::env::var("MEDIA_TRUSTED_ORIGINS"));
    if let Ok(raw) = env_val {
        for item in raw.split(',') {
            let trimmed = item.trim().trim_end_matches('/');
            if !trimmed.is_empty()
                && trimmed != primary_origin
                && !origins.iter().any(|o| o == trimmed)
                && let Ok(uri) = trimmed.parse::<axum::http::Uri>()
                && uri.scheme().is_some()
                && uri.authority().is_some()
            {
                origins.push(trimmed.to_string());
            }
        }
    }

    if let Ok(issuer) = std::env::var("MEDIA_OIDC_ISSUER")
        && let Ok(parsed) = url::Url::parse(issuer.trim())
    {
        let issuer_origin = parsed.origin().ascii_serialization();
        if issuer_origin != "null"
            && issuer_origin != primary_origin
            && !origins.iter().any(|o| o == &issuer_origin)
        {
            origins.push(issuer_origin);
        }
    }

    origins
}
