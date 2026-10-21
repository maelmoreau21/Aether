//! Application and container component versioning.
//!
//! Provides the canonical version for `aether` (standalone), `aether-core`,
//! and `aether-node`. On the `develop` branch or when built for development,
//! version evaluates to `0.1.0-beta`. On release/main, evaluates to `0.1.0`.
//! Overridable at runtime via the `MEDIA_VERSION` environment variable.

pub const BASE_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const COMPILED_VERSION: &str = env!("AETHER_APP_VERSION");

/// Returns the current active version string for this component.
pub fn current_version() -> &'static str {
    static VERSION: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    VERSION.get_or_init(|| {
        if let Ok(val) = std::env::var("MEDIA_VERSION").or_else(|_| std::env::var("AETHER_VERSION"))
        {
            let trimmed = val.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
        COMPILED_VERSION.to_string()
    })
}

#[cfg(test)]
#[path = "version_policy.rs"]
mod version_policy;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn version_is_available_with_runtime_overrides() {
        assert!(!current_version().is_empty());
        assert!(!COMPILED_VERSION.is_empty());
        assert!(!BASE_VERSION.is_empty());
    }
    #[test]
    fn develop_branch_evaluates_to_beta() {
        for branch in [
            "develop",
            "refs/heads/develop",
            "origin/develop",
            " develop ",
        ] {
            assert_eq!(
                version_policy::version_for_branch("0.2.3", Some(branch)),
                "0.2.3-beta"
            );
        }
    }
    #[test]
    fn other_branches_tags_and_detached_builds_use_base_version() {
        for branch in [
            None,
            Some("main"),
            Some("123/merge"),
            Some("HEAD"),
            Some("v0.2.3"),
            Some("feature/develop-tools"),
        ] {
            assert_eq!(version_policy::version_for_branch("0.2.3", branch), "0.2.3");
        }
    }
}
