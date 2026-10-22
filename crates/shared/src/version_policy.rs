//! Branch policy shared by the build script and deterministic unit tests.
pub fn version_for_branch(base: &str, branch: Option<&str>) -> String {
    let branch = branch.unwrap_or_default().trim();
    let branch = branch.strip_prefix("refs/heads/").unwrap_or(branch);
    let branch = branch.strip_prefix("origin/").unwrap_or(branch);
    if branch == "develop" {
        format!("{base}-beta")
    } else {
        base.to_owned()
    }
}
