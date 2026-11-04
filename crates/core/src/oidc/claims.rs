//! ID token claims and the identity attributes derived from them.
use serde::Deserialize;

#[derive(Clone, Deserialize)]
pub(super) struct Claims {
    pub(super) sub: String,
    pub(super) nonce: String,
    pub(super) iat: u64,
    pub(super) exp: u64,
    pub(super) aud: serde_json::Value,
    #[serde(default)]
    pub(super) azp: Option<String>,
    #[serde(flatten)]
    pub(super) extra: serde_json::Map<String, serde_json::Value>,
}

impl Claims {
    pub(super) fn extract_groups(&self, claim_name: &str) -> Vec<String> {
        let Some(val) = self.extra.get(claim_name) else {
            return Vec::new();
        };
        match val {
            serde_json::Value::Array(arr) => arr
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.trim().to_string()))
                .filter(|s| !s.is_empty())
                .collect(),
            serde_json::Value::String(s) => s
                .split(',')
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty())
                .collect(),
            _ => Vec::new(),
        }
    }

    pub(super) fn username(&self) -> Option<String> {
        for key in [
            "preferred_username",
            "username",
            "nickname",
            "upn",
            "unique_name",
        ] {
            if let Some(val) = self.extra.get(key).and_then(|v| v.as_str()) {
                let trimmed = val.trim();
                if !trimmed.is_empty() && trimmed.chars().count() <= 128 {
                    return Some(trimmed.to_string());
                }
            }
        }
        if let Some(email) = self.extra.get("email").and_then(|v| v.as_str())
            && let Some((local, _)) = email.split_once('@')
        {
            let trimmed = local.trim();
            if !trimmed.is_empty() && trimmed.chars().count() <= 128 {
                return Some(trimmed.to_string());
            }
        }
        None
    }

    pub(super) fn display_name(&self) -> Option<String> {
        for key in ["name", "display_name"] {
            if let Some(val) = self.extra.get(key).and_then(|v| v.as_str()) {
                let trimmed = val.trim();
                if !trimmed.is_empty() && trimmed.chars().count() <= 128 {
                    return Some(trimmed.to_string());
                }
            }
        }
        if let (Some(g), Some(f)) = (
            self.extra.get("given_name").and_then(|v| v.as_str()),
            self.extra.get("family_name").and_then(|v| v.as_str()),
        ) {
            let full = format!("{} {}", g.trim(), f.trim()).trim().to_string();
            if !full.is_empty() && full.chars().count() <= 128 {
                return Some(full);
            }
        }
        self.username()
    }
}
