//! Branch and release policy shared by the build script and deterministic unit tests.

/// Calculates the product version based on base cargo version, branch, git tag, and git commit.
pub fn version_for_build(
    base: &str,
    branch: Option<&str>,
    git_tag: Option<&str>,
    git_commit: Option<&str>,
) -> String {
    // If an exact Git tag is present and starts with 'v', use it directly.
    if let Some(tag) = git_tag {
        let trimmed = tag.trim();
        if let Some(version) = trimmed.strip_prefix('v')
            && !version.is_empty()
        {
            return version.to_string();
        }
    }

    let branch = branch.unwrap_or_default().trim();
    let branch = branch.strip_prefix("refs/heads/").unwrap_or(branch);
    let branch = branch.strip_prefix("origin/").unwrap_or(branch);

    let commit_suffix = match git_commit.map(str::trim).filter(|c| !c.is_empty()) {
        Some(sha) => format!("+{sha}"),
        None => String::new(),
    };

    if branch == "develop" {
        // On develop without an explicit tag, indicate an unreleased dev build on next beta cycle
        format!("{base}-beta.0-dev{commit_suffix}")
    } else {
        format!("{base}-dev{commit_suffix}")
    }
}

/// Fallback branch version policy for simple branch names.
#[allow(dead_code)]
pub fn version_for_branch(base: &str, branch: Option<&str>) -> String {
    version_for_build(base, branch, None, None)
}

/// Detects release channel from a version string.
pub fn channel_for_version(version: &str) -> &'static str {
    if version.contains("-dev") || version.contains('+') {
        "dev"
    } else if version.contains("-beta") {
        "beta"
    } else {
        "stable"
    }
}
