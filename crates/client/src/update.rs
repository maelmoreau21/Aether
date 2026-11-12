//! Detecting that the application is older than the server it talks to.
//!
//! The server and every native application ship in the same release, so the
//! server's version is the newest application version it knows of. Only
//! `MAJOR.MINOR.PATCH` is compared (a beta of the same number is not older).

/// Version of this application, set at build time with `AETHER_CLIENT_VERSION`
/// (release builds). `None` for a development build, which never claims to be
/// outdated.
pub const APP_VERSION: Option<&str> = option_env!("AETHER_CLIENT_VERSION");

/// Where the applications of the latest release are published.
pub const RELEASES_URL: &str = "https://github.com/maelmoreau21/Aether/releases/latest";

fn numbers(version: &str) -> Option<(u64, u64, u64)> {
    let core = version.trim().trim_start_matches('v');
    let core = core.split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let mut next = || parts.next()?.parse::<u64>().ok();
    let parsed = (next()?, next()?, next()?);
    parts.next().is_none().then_some(parsed)
}

/// `true` when `server` is a newer release than `app`. Unreadable versions are
/// never reported as outdated.
pub fn is_outdated(app: &str, server: &str) -> bool {
    matches!((numbers(app), numbers(server)), (Some(a), Some(s)) if a < s)
}

#[cfg(test)]
mod tests {
    use super::is_outdated;

    #[test]
    fn only_a_newer_release_makes_the_application_outdated() {
        assert!(is_outdated("1.0.0", "1.0.1"));
        assert!(is_outdated("0.2.9", "1.0.0"));
        assert!(is_outdated("1.2.0", "v1.10.0"));
        assert!(!is_outdated("1.0.1", "1.0.1"));
        assert!(!is_outdated("1.1.0", "1.0.9"));
    }

    #[test]
    fn prereleases_and_build_metadata_are_ignored() {
        assert!(!is_outdated("1.0.1-beta.2", "1.0.1-beta.3"));
        assert!(!is_outdated("1.0.1", "1.0.1-beta.0"));
        assert!(is_outdated("1.0.0-dev+abc", "1.0.1-beta.0"));
    }

    #[test]
    fn unreadable_versions_are_never_outdated() {
        assert!(!is_outdated("dev", "1.0.0"));
        assert!(!is_outdated("1.0.0", ""));
        assert!(!is_outdated("1.0", "1.0.1"));
        assert!(!is_outdated("1.0.0", "1.0.0.1"));
    }
}
