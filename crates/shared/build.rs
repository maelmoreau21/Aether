#[path = "src/version_policy.rs"]
mod version_policy;

fn main() {
    println!("cargo:rerun-if-env-changed=MEDIA_VERSION");
    println!("cargo:rerun-if-env-changed=AETHER_VERSION");
    println!("cargo:rerun-if-env-changed=GIT_BRANCH");
    println!("cargo:rerun-if-env-changed=GIT_COMMIT");
    println!("cargo:rerun-if-env-changed=GITHUB_REF_NAME");
    println!("cargo:rerun-if-env-changed=GITHUB_HEAD_REF");
    println!("cargo:rerun-if-env-changed=GITHUB_REF");
    println!("cargo:rerun-if-env-changed=GITHUB_SHA");
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=src/version_policy.rs");

    let base_version = env!("CARGO_PKG_VERSION");

    // Commit SHA detection
    let env_commit = std::env::var("GIT_COMMIT")
        .or_else(|_| std::env::var("GITHUB_SHA"))
        .ok()
        .map(|s| s.trim().chars().take(7).collect::<String>())
        .filter(|s| !s.is_empty());

    let git_commit = env_commit.or_else(|| {
        std::process::Command::new("git")
            .args(["rev-parse", "--short=7", "HEAD"])
            .output()
            .ok()
            .and_then(|output| {
                if output.status.success() {
                    let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    if !s.is_empty() { Some(s) } else { None }
                } else {
                    None
                }
            })
    });

    // Git tag detection
    let git_tag = std::process::Command::new("git")
        .args(["describe", "--tags", "--exact-match"])
        .output()
        .ok()
        .and_then(|output| {
            if output.status.success() {
                let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !s.is_empty() { Some(s) } else { None }
            } else {
                None
            }
        });

    // 1. Explicit version override
    let explicit_version = std::env::var("MEDIA_VERSION")
        .or_else(|_| std::env::var("AETHER_VERSION"))
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());

    // 2. Branch detection from environment (Docker build args, GitHub Actions)
    let env_branch = [
        "GIT_BRANCH",
        "GITHUB_HEAD_REF",
        "GITHUB_REF_NAME",
        "GITHUB_REF",
    ]
    .into_iter()
    .filter_map(|name| std::env::var(name).ok())
    .map(|value| value.trim().to_owned())
    .find(|value| !value.is_empty());

    // 3. Branch detection from local git repository
    let branch = env_branch
        .or_else(|| {
            std::process::Command::new("git")
                .args(["rev-parse", "--abbrev-ref", "HEAD"])
                .output()
                .ok()
                .and_then(|output| {
                    if output.status.success() {
                        let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
                        if !name.is_empty() && name != "HEAD" {
                            Some(name)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
        })
        .or_else(|| {
            let head_paths = [
                std::path::PathBuf::from("../../.git/HEAD"),
                std::path::PathBuf::from(".git/HEAD"),
            ];
            for path in &head_paths {
                if let Ok(head) = std::fs::read_to_string(path) {
                    let trimmed = head.trim();
                    if let Some(ref_name) = trimmed.strip_prefix("ref: refs/heads/") {
                        return Some(ref_name.trim().to_string());
                    }
                }
            }
            None
        });

    let computed_version = match explicit_version {
        Some(explicit) => explicit,
        None => version_policy::version_for_build(
            base_version,
            branch.as_deref(),
            git_tag.as_deref(),
            git_commit.as_deref(),
        ),
    };

    let channel = version_policy::channel_for_version(&computed_version);
    let commit_str = git_commit.unwrap_or_else(|| "unknown".to_string());

    // Build date
    let build_date = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|epoch| epoch.trim().parse::<i64>().ok())
        .map(|sec| {
            // Rough format or epoch representation
            format!("{sec}")
        })
        .unwrap_or_else(|| {
            // Static or current format
            "2026-10-26".to_string()
        });

    println!("cargo:rustc-env=AETHER_APP_VERSION={computed_version}");
    println!("cargo:rustc-env=AETHER_GIT_COMMIT={commit_str}");
    println!("cargo:rustc-env=AETHER_BUILD_CHANNEL={channel}");
    println!("cargo:rustc-env=AETHER_BUILD_DATE={build_date}");
}
