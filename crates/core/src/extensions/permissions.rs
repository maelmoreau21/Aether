//! Single source of truth for extension permissions, operation mapping and default library scope.

/// Closed list of permissions an extension manifest may declare.
pub(super) const ALLOWED_PERMISSIONS: &[&str] = &[
    "metadata.read",
    "metadata.write",
    "avatars",
    "images",
    "providers",
    "storage.private",
    "playback.markers",
    "subtitles",
    "requests",
    "reports",
];

pub(super) fn is_allowed(permission: &str) -> bool {
    ALLOWED_PERMISSIONS.contains(&permission)
}

/// Maps an extension operation name to the manifest permission required to invoke it.
pub(crate) fn permission_for_operation(operation: &str) -> Option<&'static str> {
    match operation {
        "metadata.propose" | "metadata.search" | "metadata.details" | "lyrics.get" => {
            Some("metadata.read")
        }
        "metadata.images" => Some("images"),
        "subtitles.search" | "subtitles.download" => Some("subtitles"),
        "avatars.list" | "avatars.generate" => Some("avatars"),
        "requests.validate" | "requests.plan" => Some("requests"),
        "reports.validate" | "reports.classify" => Some("reports"),
        "intro.detect"
        | "markers.detect"
        | "intro.match_fingerprints"
        | "intro.parse_chapters"
        | "markers.get" => Some("playback.markers"),
        _ => None,
    }
}

/// Whether `operation` belongs to the named native application capability (`requests` or `reports`).
pub(crate) fn is_application_operation(capability: &str, operation: &str) -> bool {
    matches!(capability, "requests" | "reports")
        && permission_for_operation(operation) == Some(capability)
}

/// Initial library scope of a fresh install. Metadata providers and the native
/// application surfaces (reports, requests) apply to every library: the caller's
/// own library permissions still decide what each user may reach. Without this,
/// an application extension starts with an empty scope and refuses every call
/// with `library_permission_denied`.
pub(super) fn default_all_libraries(permissions: &[String]) -> bool {
    permissions
        .iter()
        .any(|permission| matches!(permission.as_str(), "metadata.read" | "reports" | "requests"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operation_permissions_are_all_declared_allowed() {
        let ops = [
            "metadata.propose",
            "metadata.search",
            "metadata.details",
            "lyrics.get",
            "metadata.images",
            "subtitles.search",
            "subtitles.download",
            "avatars.list",
            "avatars.generate",
            "requests.validate",
            "requests.plan",
            "reports.validate",
            "reports.classify",
            "intro.detect",
            "markers.detect",
            "intro.match_fingerprints",
            "intro.parse_chapters",
            "markers.get",
        ];
        for op in ops {
            let perm = permission_for_operation(op).expect("known operation");
            assert!(is_allowed(perm), "permission {perm} must be allowed");
        }
        assert_eq!(permission_for_operation("unknown.op"), None);
    }

    #[test]
    fn application_operations_match_capability_and_permission() {
        assert!(is_application_operation("requests", "requests.plan"));
        assert!(is_application_operation("requests", "requests.validate"));
        assert!(is_application_operation("reports", "reports.classify"));
        assert!(is_application_operation("reports", "reports.validate"));
        assert!(!is_application_operation("requests", "reports.classify"));
        assert!(!is_application_operation("metadata.read", "metadata.search"));
    }

    #[test]
    fn default_scope_applies_only_to_global_surfaces() {
        assert!(default_all_libraries(&["metadata.read".into()]));
        assert!(default_all_libraries(&["reports".into()]));
        assert!(default_all_libraries(&["requests".into()]));
        assert!(!default_all_libraries(&["playback.markers".into()]));
        assert!(!default_all_libraries(&["subtitles".into()]));
    }
}
