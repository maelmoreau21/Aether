use std::path::Path;

pub(crate) fn ignored(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.starts_with('.')
        || name.starts_with('@')
        || name.starts_with("#recycle")
        || name.starts_with("#snapshot")
        || name.ends_with(".trickplay")
        || name.ends_with(".metathumb")
        || matches!(
            name.as_str(),
            "extrafanart"
                | "$recycle.bin"
                | "system volume information"
                | "lost+found"
                | "#recycle"
                | "#snapshot"
                | ".@__thumb"
                | "thumbs.db"
                | "desktop.ini"
        )
        || [
            ".part",
            ".partial",
            ".tmp",
            ".temp",
            ".crdownload",
            ".download",
            ".!qb",
        ]
        .iter()
        .any(|suffix| name.ends_with(suffix))
}

pub(crate) fn is_excluded(relative_path: &str, name: &str, excluded_patterns: &[String]) -> bool {
    if ignored(name) {
        return true;
    }
    let lower_name = name.to_ascii_lowercase();
    let lower_rel = relative_path.to_ascii_lowercase();
    for pat in excluded_patterns {
        let pat = pat.trim().to_ascii_lowercase();
        if pat.is_empty() {
            continue;
        }
        if lower_name == pat || lower_rel.split('/').any(|seg| seg == pat) {
            return true;
        }
    }
    false
}

pub(crate) fn directory_is_ignored(path: &Path) -> bool {
    [".nomedia", ".ignore", ".aetherignore"]
        .iter()
        .any(|marker| path.join(marker).exists())
}
