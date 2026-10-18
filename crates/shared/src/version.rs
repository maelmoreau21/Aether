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
mod tests {
    use super::*;

    #[test]
    fn test_version_not_empty_and_starts_with_base() {
        let v = current_version();
        assert!(!v.is_empty());
        assert!(v.starts_with(BASE_VERSION));
    }

    #[test]
    fn test_develop_branch_evaluates_to_beta() {
        // In local git checkout on develop branch, the compiled version must end with -beta
        let v = COMPILED_VERSION;
        assert_eq!(v, "0.1.0-beta");
    }
}
