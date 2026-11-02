//! Application and container component versioning.
//!
//! Provides the canonical version for `aether` (standalone), `aether-core`,
//! and `aether-node`. On the `develop` branch or when built for development,
//! version evaluates to a beta/dev string (e.g. `0.2.0-beta.0-dev`).
//! On release/main, evaluates to stable SemVer (e.g. `0.1.0`, `0.2.0`).
//! Overridable at build time via `MEDIA_VERSION` / `AETHER_VERSION` or at
//! runtime via the `MEDIA_VERSION` environment variable.

pub const BASE_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const COMPILED_VERSION: &str = env!("AETHER_APP_VERSION");
pub const COMPILED_COMMIT: &str = env!("AETHER_GIT_COMMIT");
pub const COMPILED_CHANNEL: &str = env!("AETHER_BUILD_CHANNEL");
pub const COMPILED_BUILD_DATE: &str = env!("AETHER_BUILD_DATE");

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct VersionInfo {
    pub version: String,
    pub channel: String,
    pub commit: String,
    pub build_date: String,
    pub release_url: Option<String>,
}

/// Returns the structured version details for the running instance.
pub fn current_version_info() -> VersionInfo {
    let effective_version = std::env::var("MEDIA_VERSION")
        .or_else(|_| std::env::var("AETHER_VERSION"))
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| COMPILED_VERSION.to_string());

    let channel = if effective_version.contains("-dev") || effective_version.contains('+') {
        "dev".to_string()
    } else if effective_version.contains("-beta") {
        "beta".to_string()
    } else {
        "stable".to_string()
    };

    let release_url = if channel == "stable" || channel == "beta" {
        let tag = format!("v{effective_version}");
        Some(format!(
            "https://github.com/maelmoreau21/Aether/releases/tag/{tag}"
        ))
    } else {
        None
    };

    let commit = std::env::var("MEDIA_COMMIT")
        .or_else(|_| std::env::var("AETHER_COMMIT"))
        .ok()
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty())
        .unwrap_or_else(|| COMPILED_COMMIT.to_string());

    VersionInfo {
        version: effective_version,
        channel,
        commit,
        build_date: COMPILED_BUILD_DATE.to_string(),
        release_url,
    }
}

/// Returns the current active version string for this component.
pub fn current_version() -> &'static str {
    static VERSION: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    VERSION.get_or_init(|| current_version_info().version)
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
        let info = current_version_info();
        assert!(!info.version.is_empty());
        assert!(!info.channel.is_empty());
    }

    #[test]
    fn develop_branch_evaluates_to_dev_or_beta() {
        for branch in [
            "develop",
            "refs/heads/develop",
            "origin/develop",
            " develop ",
        ] {
            let ver = version_policy::version_for_branch("0.2.0", Some(branch));
            assert!(ver.starts_with("0.2.0-beta.0-dev"));
            assert_eq!(version_policy::channel_for_version(&ver), "dev");
        }
    }

    #[test]
    fn exact_tag_takes_precedence_in_build() {
        let ver = version_policy::version_for_build(
            "0.2.0",
            Some("develop"),
            Some("v0.2.0-beta.3"),
            Some("b08c635"),
        );
        assert_eq!(ver, "0.2.0-beta.3");
        assert_eq!(version_policy::channel_for_version(&ver), "beta");
    }

    #[test]
    fn stable_tag_gives_stable_channel() {
        let ver = version_policy::version_for_build(
            "0.1.0",
            Some("main"),
            Some("v0.1.0"),
            Some("bb99548"),
        );
        assert_eq!(ver, "0.1.0");
        assert_eq!(version_policy::channel_for_version(&ver), "stable");
    }

    #[test]
    fn other_branches_and_detached_builds_use_dev_channel() {
        for branch in [
            None,
            Some("main"),
            Some("123/merge"),
            Some("HEAD"),
            Some("feature/tools"),
        ] {
            let ver = version_policy::version_for_build("0.2.0", branch, None, Some("abc1234"));
            assert_eq!(ver, "0.2.0-dev+abc1234");
            assert_eq!(version_policy::channel_for_version(&ver), "dev");
        }
    }
}
