fn main() {
    println!("cargo:rerun-if-env-changed=MEDIA_VERSION");
    println!("cargo:rerun-if-env-changed=AETHER_VERSION");
    println!("cargo:rerun-if-env-changed=GIT_BRANCH");
    println!("cargo:rerun-if-env-changed=GITHUB_REF_NAME");
    println!("cargo:rerun-if-env-changed=GITHUB_REF");
    println!("cargo:rerun-if-changed=../../.git/HEAD");

    let base_version = env!("CARGO_PKG_VERSION");

    // 1. Explicit version override
    if let Ok(explicit) =
        std::env::var("MEDIA_VERSION").or_else(|_| std::env::var("AETHER_VERSION"))
    {
        let trimmed = explicit.trim();
        if !trimmed.is_empty() {
            println!("cargo:rustc-env=AETHER_APP_VERSION={trimmed}");
            return;
        }
    }

    // 2. Branch detection from environment (Docker build args, GitHub Actions)
    let env_branch = std::env::var("GIT_BRANCH")
        .or_else(|_| std::env::var("GITHUB_REF_NAME"))
        .or_else(|_| {
            std::env::var("GITHUB_REF").map(|ref_str| {
                ref_str
                    .strip_prefix("refs/heads/")
                    .unwrap_or(&ref_str)
                    .to_string()
            })
        })
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

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
            // Direct .git/HEAD inspection if git binary is absent
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

    let is_develop = match branch.as_deref() {
        Some("develop") => true,
        Some(b) if b.contains("develop") => true,
        _ => false,
    };

    let computed_version = if is_develop {
        format!("{base_version}-beta")
    } else {
        base_version.to_string()
    };

    println!("cargo:rustc-env=AETHER_APP_VERSION={computed_version}");
}
