//! What survives a restart. Plain settings (the server address, this device's
//! remote-control id) go to a JSON file in the user's configuration folder;
//! the device token goes to the system keychain only (Windows Credential
//! Manager, macOS Keychain, Secret Service on Linux) through GPUI.
use aether_client::Token;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const SETTINGS_FILE: &str = "settings.json";

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// The last Core signed in to; its token is in the keychain under this URL.
    #[serde(default)]
    pub server: Option<String>,
    /// Stable id of this computer on the remote-control channel.
    #[serde(default)]
    pub device_id: Option<String>,
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
    /// The remote-control id, created on first use.
    pub fn device_id(&mut self) -> String {
        self.device_id
            .get_or_insert_with(|| uuid::Uuid::new_v4().to_string())
            .clone()
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

/// This computer's name in the account's sessions and on the remote control.
pub fn device_name(var: impl Fn(&str) -> Option<String>) -> String {
    ["COMPUTERNAME", "HOSTNAME"]
        .iter()
        .find_map(|name| var(name))
        .map(|name| name.trim().chars().take(48).collect::<String>())
        .filter(|name| !name.is_empty() && name.chars().all(|c| !c.is_control()))
        .unwrap_or_else(|| "Aether Desktop".to_owned())
}

/// Keychain entry key for a server: one entry per Core.
pub fn keychain_url(server: &str) -> String {
    format!("{}#aether-desktop", server.trim_end_matches('/'))
}

/// The keychain stores the token as JSON bytes.
pub fn encode_token(token: &Token) -> Vec<u8> {
    serde_json::to_vec(token).unwrap_or_default()
}
pub fn decode_token(bytes: &[u8]) -> Option<Token> {
    serde_json::from_slice(bytes)
        .ok()
        .filter(|t: &Token| !t.access.is_empty())
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
    fn settings_round_trip_and_keep_a_stable_device_id() {
        let dir = std::env::temp_dir().join(format!("aether-desktop-{}", uuid::Uuid::new_v4()));
        assert_eq!(Settings::load(&dir), Settings::default());
        let mut settings = Settings {
            server: Some("https://media.example".into()),
            device_id: None,
        };
        let id = settings.device_id();
        assert_eq!(settings.device_id(), id);
        settings.save(&dir).unwrap();
        assert_eq!(Settings::load(&dir), settings);
        std::fs::write(dir.join(SETTINGS_FILE), b"{broken").unwrap();
        assert_eq!(Settings::load(&dir), Settings::default());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn folders_names_and_tokens() {
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
            keychain_url("https://media.example/"),
            "https://media.example#aether-desktop"
        );
        let token = Token {
            access: "secret".into(),
            expires_at: 42,
        };
        assert_eq!(decode_token(&encode_token(&token)), Some(token));
        assert_eq!(decode_token(br#"{"access":"","expires_at":1}"#), None);
        assert_eq!(decode_token(b"garbage"), None);
        assert_eq!(
            normalize_server(" media.example/ "),
            "https://media.example"
        );
        assert_eq!(
            normalize_server("http://10.0.0.2:8080/"),
            "http://10.0.0.2:8080"
        );
    }
}
