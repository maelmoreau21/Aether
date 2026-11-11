//! What survives a restart: the address of the server, in a JSON file of the
//! user's configuration folder. The session itself is the web interface's
//! cookie, kept by the web engine in the `webview` folder next to it.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const SETTINGS_FILE: &str = "settings.json";

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// The Core opened at start-up.
    #[serde(default)]
    pub server: Option<String>,
    /// The window as it was left.
    #[serde(default)]
    pub window: Option<WindowState>,
}

/// Size (logical pixels) of the window when it is not maximized.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowState {
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub maximized: bool,
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 800,
            maximized: false,
        }
    }
}

impl WindowState {
    /// A size the window can really take, whatever the file says.
    pub fn size(self) -> (f64, f64) {
        (
            f64::from(self.width.clamp(480, 7680)),
            f64::from(self.height.clamp(480, 4320)),
        )
    }
}

impl Settings {
    pub fn load(dir: &Path) -> Self {
        std::fs::read(dir.join(SETTINGS_FILE))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }
    /// Written next to the final file then renamed, so a crash never leaves
    /// half a file.
    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let temporary = dir.join(format!("{SETTINGS_FILE}.tmp"));
        std::fs::write(&temporary, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(temporary, dir.join(SETTINGS_FILE))
    }
}

/// The per-user configuration folder: `%APPDATA%\Aether`,
/// `~/Library/Application Support/Aether`, `$XDG_CONFIG_HOME/aether`.
pub fn config_dir(var: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    let set = |name: &str| var(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    if cfg!(windows) {
        return set("APPDATA").map(|d| d.join("Aether"));
    }
    if cfg!(target_os = "macos") {
        return set("HOME").map(|h| h.join("Library/Application Support/Aether"));
    }
    set("XDG_CONFIG_HOME")
        .or_else(|| set("HOME").map(|h| h.join(".config")))
        .map(|d| d.join("aether"))
}

/// This computer's name, sent with the request that identifies the server.
pub fn device_name(var: impl Fn(&str) -> Option<String>) -> String {
    ["COMPUTERNAME", "HOSTNAME"]
        .iter()
        .find_map(|name| var(name))
        .map(|name| name.trim().chars().take(48).collect::<String>())
        .filter(|name| !name.is_empty() && name.chars().all(|c| !c.is_control()))
        .unwrap_or_else(|| "Aether Desktop".to_owned())
}

/// Normalizes what the person typed: adds `https://` when no scheme is given
/// and drops a trailing slash. Validation is left to the client core.
pub fn normalize_server(input: &str) -> String {
    let trimmed = input.trim().trim_end_matches('/');
    if trimmed.contains("://") {
        trimmed.to_owned()
    } else {
        format!("https://{trimmed}")
    }
}

/// What was downloaded from a Core, readable without it (the web interface
/// keeps this page and the encrypted media in the web engine's storage).
pub fn downloads_url(server: &str) -> String {
    format!("{}downloads/", app_url(server))
}

/// The web interface of a Core.
pub fn app_url(server: &str) -> String {
    format!("{}/app/", server.trim_end_matches('/'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |key| map.get(key).cloned()
    }

    #[test]
    fn settings_round_trip() {
        let dir = std::env::temp_dir().join(format!("aether-desktop-{}", std::process::id()));
        assert_eq!(Settings::load(&dir), Settings::default());
        let mut settings = Settings {
            server: Some("https://media.example".into()),
            window: Some(WindowState {
                width: 1600,
                height: 900,
                maximized: true,
            }),
        };
        settings.save(&dir).unwrap();
        assert_eq!(Settings::load(&dir), settings);
        settings.window = None;
        let tiny = WindowState {
            width: 1,
            height: 99999,
            maximized: false,
        };
        assert_eq!(tiny.size(), (480.0, 4320.0));
        // A file written by an earlier version keeps working.
        std::fs::write(
            dir.join(SETTINGS_FILE),
            br#"{"server":"https://media.example","deviceId":"old"}"#,
        )
        .unwrap();
        assert_eq!(Settings::load(&dir), settings);
        std::fs::write(dir.join(SETTINGS_FILE), b"{broken").unwrap();
        assert_eq!(Settings::load(&dir), Settings::default());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn folders_names_and_addresses() {
        let dir = config_dir(env(&[
            ("APPDATA", "C:\\Users\\a\\AppData\\Roaming"),
            ("HOME", "/home/a"),
            ("XDG_CONFIG_HOME", ""),
        ]))
        .unwrap();
        if cfg!(windows) {
            assert!(dir.ends_with("Aether"));
        } else if cfg!(target_os = "macos") {
            assert_eq!(
                dir,
                PathBuf::from("/home/a/Library/Application Support/Aether")
            );
        } else {
            assert_eq!(dir, PathBuf::from("/home/a/.config/aether"));
        }
        assert_eq!(config_dir(env(&[])), None);
        assert_eq!(device_name(env(&[("HOSTNAME", " salon ")])), "salon");
        assert_eq!(device_name(env(&[("HOSTNAME", "a\nb")])), "Aether Desktop");
        assert_eq!(device_name(env(&[])), "Aether Desktop");
        assert_eq!(
            normalize_server(" media.example/ "),
            "https://media.example"
        );
        assert_eq!(
            normalize_server("http://10.0.0.2:8080/"),
            "http://10.0.0.2:8080"
        );
        assert_eq!(
            app_url("https://media.example/"),
            "https://media.example/app/"
        );
    }
}
