//! Account-owned playback/UI preferences, included in ordinary database backups.
use crate::auth::AuthState;
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    routing::get,
};
use serde::{Deserialize, Serialize};

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Preferences {
    #[serde(skip_serializing_if = "Option::is_none")]
    theme: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    locale: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    home_sections: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    card_size: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    card_density: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    accent_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    audio_language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    remember_audio_selections: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    remember_subtitle_selections: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    play_default_audio_track: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hide_played_in_latest: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    subtitles: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    audio_track: Option<AudioTrack>,
    #[serde(skip_serializing_if = "Option::is_none")]
    offsets: Option<Offsets>,
    #[serde(skip_serializing_if = "Option::is_none")]
    subtitle_preference: Option<SubtitlePreference>,
    #[serde(skip_serializing_if = "Option::is_none")]
    subtitle_style: Option<SubtitleStyle>,
    #[serde(skip_serializing_if = "Option::is_none")]
    skip_intro_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    skip_credits_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    auto_skip_intro: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    auto_skip_credits: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    skip_recap_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    auto_skip_recap: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    autoplay_next_episode: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    skip_back_seconds: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    skip_forward_seconds: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    artwork_rotation: Option<ArtworkRotation>,
}
fn default_true() -> bool {
    true
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ArtworkRotation {
    enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    language_mode: Option<String>,
    #[serde(default = "default_true")]
    enable_movies: bool,
    #[serde(default = "default_true")]
    enable_series: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    enabled_libraries: Option<Vec<String>>,
    rotation_mode: String,
    min_hours_between_switches: u32,
    pool_size: u32,
    preferred_language: String,
    fallback_language: String,
    include_unknown_language: bool,
    allow_any_language_fallback: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AudioTrack {
    media_id: String,
    track_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    language: Option<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Offsets {
    audio: f64,
    subtitle: f64,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SubtitleStyle {
    #[serde(skip_serializing_if = "Option::is_none")]
    font_size: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    font_family: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    background_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    background_opacity: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text_shadow: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SubtitlePreference {
    enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    forced: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sdh: Option<bool>,
}
fn language(value: &str) -> bool {
    (2..=35).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
}
fn hex_color(value: &str) -> bool {
    value.len() == 7 && value.starts_with('#') && value[1..].bytes().all(|b| b.is_ascii_hexdigit())
}
impl Preferences {
    fn valid(&self) -> bool {
        self.home_sections.as_ref().is_none_or(|sections| {
            sections.len() <= 6
                && sections.iter().enumerate().all(|(index, section)| {
                    matches!(
                        section.as_str(),
                        "hero" | "continue" | "tonight" | "films" | "series" | "rated"
                    ) && !sections[..index].contains(section)
                })
        }) && self
            .card_size
            .as_deref()
            .is_none_or(|v| matches!(v, "small" | "medium" | "large"))
            && self
                .card_density
                .as_deref()
                .is_none_or(|v| matches!(v, "comfortable" | "compact"))
            && self
                .accent_color
                .as_deref()
                .is_none_or(|v| matches!(v, "orange" | "blue" | "violet" | "green"))
            && self
                .skip_back_seconds
                .is_none_or(|v| matches!(v, 5 | 10 | 15 | 30 | 60))
            && self
                .skip_forward_seconds
                .is_none_or(|v| matches!(v, 5 | 10 | 15 | 30 | 60))
            && self.artwork_rotation.as_ref().is_none_or(|v| {
                v.language_mode
                    .as_deref()
                    .is_none_or(|mode| matches!(mode, "interface" | "original"))
                    && matches!(v.rotation_mode.as_str(), "random" | "sequential")
                    && v.min_hours_between_switches <= 720
                    && (1..=24).contains(&v.pool_size)
                    && language(&v.preferred_language)
                    && language(&v.fallback_language)
                    && v.enabled_libraries.as_ref().is_none_or(|libraries| {
                        libraries.len() <= 64
                            && libraries.iter().all(|lib| {
                                (1..=64).contains(&lib.len())
                                    && lib
                                        .bytes()
                                        .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
                            })
                    })
            })
            && self
                .theme
                .as_deref()
                .is_none_or(|v| matches!(v, "light" | "dark" | "system"))
            && self
                .locale
                .as_deref()
                .is_none_or(|v| matches!(v, "fr" | "en"))
            && self.audio_language.as_deref().is_none_or(language)
            && self
                .subtitles
                .as_deref()
                .is_none_or(|v| matches!(v, "forced" | "always" | "off"))
            && self.audio_track.as_ref().is_none_or(|v| {
                v.language.as_deref().is_none_or(language)
                    && [&v.media_id, &v.track_id].into_iter().all(|s| {
                        !s.is_empty() && s.len() <= 256 && !s.chars().any(char::is_control)
                    })
            })
            && self.offsets.as_ref().is_none_or(|v| {
                [v.audio, v.subtitle]
                    .into_iter()
                    .all(|v| v.is_finite() && v.abs() <= 30.)
            })
            && self
                .subtitle_preference
                .as_ref()
                .is_none_or(|v| v.language.as_deref().is_none_or(language))
            && self.subtitle_style.as_ref().is_none_or(|s| {
                s.font_size
                    .as_deref()
                    .is_none_or(|v| matches!(v, "small" | "medium" | "large"))
                    && s.font_family
                        .as_deref()
                        .is_none_or(|v| matches!(v, "sans-serif" | "serif" | "monospace"))
                    && s.text_color.as_deref().is_none_or(hex_color)
                    && s.background_color.as_deref().is_none_or(hex_color)
                    && s.background_opacity
                        .is_none_or(|v| v.is_finite() && (0.0..=100.0).contains(&v))
                    && s.text_shadow
                        .as_deref()
                        .is_none_or(|v| matches!(v, "none" | "outline" | "drop-shadow"))
            })
    }
}
pub fn router(auth: AuthState) -> Router {
    Router::new()
        .route("/api/v1/preferences", get(read).put(write))
        .layer(DefaultBodyLimit::max(8192))
        .with_state(auth)
}
async fn read(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Result<Json<Preferences>, StatusCode> {
    let identity = auth.authenticate(&headers).await?;
    let saved: Option<String> = crate::database::query_scalar(
        "SELECT preferences_json FROM user_preferences WHERE user_id=?",
    )
    .bind(identity.user.id)
    .fetch_optional(&auth.store.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(
        saved.map(|value| tolerant(&value)).unwrap_or_default(),
    ))
}

/// Stored preferences outlive client versions: a field that no longer validates is
/// dropped on its own instead of making every preference unavailable.
fn tolerant(stored: &str) -> Preferences {
    if let Ok(preferences) = serde_json::from_str::<Preferences>(stored)
        && preferences.valid()
    {
        return preferences;
    }
    let Ok(serde_json::Value::Object(fields)) = serde_json::from_str(stored) else {
        return Preferences::default();
    };
    let mut kept = serde_json::Map::new();
    for (key, value) in fields {
        let single =
            serde_json::Value::Object([(key.clone(), value.clone())].into_iter().collect());
        if serde_json::from_value::<Preferences>(single).is_ok_and(|p| p.valid()) {
            kept.insert(key, value);
        }
    }
    serde_json::from_value(serde_json::Value::Object(kept)).unwrap_or_default()
}
async fn write(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<Preferences>,
) -> Result<StatusCode, StatusCode> {
    let identity = auth.authenticate(&headers).await?;
    auth.require_csrf(&headers, &identity)?;
    if !input.valid() {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }
    let mut conn = auth
        .store
        .pool
        .acquire()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut tx = crate::database::begin_write(&mut conn)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let previous: Option<String> = crate::database::query_scalar(
        "SELECT preferences_json FROM user_preferences WHERE user_id=?",
    )
    .bind(&identity.user.id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    // Rebuilt from the readable fields only, so one stale value never blocks saving.
    let mut saved =
        serde_json::to_value(previous.map(|value| tolerant(&value)).unwrap_or_default())
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let patch = serde_json::to_value(input).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let target = saved
        .as_object_mut()
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    if let Some(fields) = patch.as_object() {
        for (key, value) in fields {
            target.insert(key.clone(), value.clone());
        }
    }
    let encoded = serde_json::to_string(&saved).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if encoded.len() > 4096 {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }
    crate::database::query("INSERT INTO user_preferences(user_id,preferences_json) VALUES(?,?) ON CONFLICT(user_id) DO UPDATE SET preferences_json=excluded.preferences_json").bind(identity.user.id).bind(encoded).execute(&mut *tx).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    tx.commit()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stale_field_no_longer_hides_every_preference() {
        let kept =
            tolerant(r#"{"theme":"dark","removedField":1,"cardSize":"huge","autoSkipIntro":true}"#);
        assert_eq!(kept.theme.as_deref(), Some("dark"));
        assert_eq!(kept.card_size, None);
        assert_eq!(kept.auto_skip_intro, Some(true));
        assert!(tolerant("not json").theme.is_none());
    }
}
