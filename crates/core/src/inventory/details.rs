//! Read-only technical sheet of one indexed asset for the "media information" panel.
//!
//! The asset is resolved from the inventory by its opaque identifier only: no path is
//! ever accepted from the client. Library-relative and Node-side absolute paths are
//! disclosed to administrators exclusively (`include_paths`); other users receive the
//! Media Node identity, the library and the technical description without any path.

use super::decode;
use crate::database::query;
use crate::identity::IdentityStore;
use serde_json::{Map, Value, json};
use sqlx::Row;

/// Stream fields copied from the probe; everything else (side data, extradata,
/// arbitrary tags) stays private to avoid leaking unreviewed metadata.
const STREAM_FIELDS: &[&str] = &[
    "codec_long_name",
    "profile",
    "level",
    "width",
    "height",
    "pix_fmt",
    "color_space",
    "color_transfer",
    "color_primaries",
    "color_range",
    "field_order",
    "sample_rate",
    "channels",
    "channel_layout",
    "bits_per_raw_sample",
];
const DISPOSITIONS: &[&str] = &[
    "default",
    "forced",
    "comment",
    "hearing_impaired",
    "visual_impaired",
    "original",
    "dub",
];

pub async fn asset_details(
    store: &IdentityStore,
    asset_id: &str,
    include_paths: bool,
) -> Result<Option<Value>, sqlx::Error> {
    crate::nodes::ensure_node_registry_warm(store).await?;
    let row = query(
        "SELECT a.id AS asset_id, a.relative_path, CAST(a.size AS BIGINT) AS size, a.modified_ns, a.mime, a.probe_json, \
         CAST(a.available AS BIGINT) AS available, l.root, l.node_id, l.kind AS library_kind, \
         COALESCE(parent.id, l.id) AS library_id, COALESCE(parent.name, l.name) AS library_name, \
         n.name AS node_name, n.kind AS node_kind, n.status AS node_status, n.version AS node_version, \
         e.work_id, t.subtitles_json, au.audios_json \
         FROM inventory_assets a \
         JOIN inventory_sources s ON s.id=a.source_id \
         JOIN inventory_versions v ON v.id=s.version_id \
         JOIN inventory_editions e ON e.id=v.edition_id \
         JOIN libraries l ON l.id=a.library_id \
         JOIN media_nodes n ON n.id=l.node_id \
         LEFT JOIN library_folders f ON f.library_id=l.id AND f.active=1 \
         LEFT JOIN libraries parent ON parent.id=f.parent_id \
         LEFT JOIN inventory_subtitles t ON t.asset_id=a.id \
         LEFT JOIN inventory_audio au ON au.asset_id=a.id \
         WHERE a.id=? AND s.root_snapshot=l.root AND l.kind IN ('films', 'series') AND n.status != 'revoked'",
    )
    .bind(asset_id)
    .fetch_optional(&store.pool)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };

    let mime: String = row.get("mime");
    let node_id: String = row.get("node_id");
    let (node_status, connection_mode) = match store.node_registry.get(&node_id) {
        Some(entry) => {
            let (status, mode, _) = crate::nodes::score_node_entry(
                &entry,
                Some(&mime),
                None,
                media_contracts::protocol::unix_now(),
            );
            (status, Some(mode))
        }
        None => (row.get::<String, _>("node_status"), None),
    };
    let probe = decode(row.get("probe_json"))?;
    let relative_path: String = row.get("relative_path");
    let root: String = row.get("root");

    let mut node = json!({
        "id": node_id,
        "name": row.get::<String, _>("node_name"),
        "kind": row.get::<String, _>("node_kind"),
        "status": node_status,
        "connectionMode": connection_mode,
    });
    let mut file = json!({
        "size": row.get::<i64, _>("size"),
        "mime": mime,
        "modifiedAt": modified_seconds(row.get::<String, _>("modified_ns").as_str()),
    });
    if include_paths {
        node["version"] = json!(row.get::<Option<String>, _>("node_version"));
        file["name"] = json!(file_name(&relative_path));
        file["relativePath"] = json!(relative_path);
        file["libraryRoot"] = json!(root);
        file["absolutePath"] = json!(join_node_path(&root, &relative_path));
    }

    let tracks: Vec<Value> = probe.streams.iter().map(track_summary).collect();

    let sidecar_subtitles: Vec<media_contracts::inventory::ExternalSubtitle> =
        serde_json::from_str(
            row.get::<Option<String>, _>("subtitles_json")
                .as_deref()
                .unwrap_or("[]"),
        )
        .map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
    let mut external_subtitles: Vec<Value> = sidecar_subtitles
        .iter()
        .map(|track| {
            json!({
                "origin": "sidecar", "language": track.language, "label": track.label,
                "forced": track.forced, "sdh": track.sdh, "sourceFormat": track.source_format,
            })
        })
        .collect();
    for downloaded in crate::extensions::subtitles::descriptors(store, asset_id).await? {
        external_subtitles.push(json!({
            "origin": "downloaded",
            "language": downloaded.get("language").cloned().unwrap_or(Value::Null),
            "label": downloaded.get("label").cloned().unwrap_or(Value::Null),
            "forced": downloaded.get("forced").and_then(Value::as_bool).unwrap_or(false),
            "sdh": downloaded.get("sdh").and_then(Value::as_bool).unwrap_or(false),
            "sourceFormat": "vtt",
        }));
    }

    let sidecar_audio: Vec<media_contracts::inventory::ExternalAudio> = serde_json::from_str(
        row.get::<Option<String>, _>("audios_json")
            .as_deref()
            .unwrap_or("[]"),
    )
    .map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
    let external_audio: Vec<Value> = sidecar_audio
        .iter()
        .map(|audio| {
            let track = audio
                .probe
                .streams
                .iter()
                .find(|track| track.codec_type.as_deref() == Some("audio"));
            let mut value = json!({
                "language": audio.language,
                "size": audio.size,
                "offsetSeconds": audio.offset_seconds,
                "track": track.map(track_summary),
            });
            if include_paths {
                value["relativePath"] = json!(audio.relative_path);
                value["absolutePath"] = json!(join_node_path(&root, &audio.relative_path));
            }
            value
        })
        .collect();

    Ok(Some(json!({
        "assetId": row.get::<String, _>("asset_id"),
        "workId": row.get::<String, _>("work_id"),
        "available": row.get::<i64, _>("available") == 1,
        "pathsVisible": include_paths,
        "node": node,
        "library": {
            "id": row.get::<String, _>("library_id"),
            "name": row.get::<String, _>("library_name"),
            "kind": row.get::<String, _>("library_kind"),
        },
        "file": file,
        "container": container_summary(&probe.format),
        "tracks": tracks,
        "externalAudio": external_audio,
        "externalSubtitles": external_subtitles,
        "chapterCount": probe.chapters.len(),
    })))
}

fn container_summary(format: &std::collections::BTreeMap<String, Value>) -> Value {
    // `filename` and private scanner markers are intentionally never copied.
    json!({
        "formatName": format.get("format_name").and_then(Value::as_str),
        "formatLongName": format.get("format_long_name").and_then(Value::as_str),
        "durationSeconds": format.get("duration").and_then(number),
        "bitRate": format.get("bit_rate").and_then(number).map(|v| v.round() as u64),
        "startTime": format.get("start_time").and_then(number),
    })
}

fn track_summary(track: &media_node::probe::Track) -> Value {
    let mut value = Map::new();
    value.insert("index".into(), json!(track.index));
    value.insert("codecType".into(), json!(track.codec_type));
    value.insert("codecName".into(), json!(track.codec_name));
    for field in STREAM_FIELDS {
        if let Some(detail) = track.details.get(*field).filter(|v| !v.is_null()) {
            value.insert(camel(field), detail.clone());
        }
    }
    let rate = track
        .details
        .get("avg_frame_rate")
        .and_then(frame_rate)
        .or_else(|| track.details.get("r_frame_rate").and_then(frame_rate));
    if let Some(rate) = rate {
        value.insert("frameRate".into(), json!(rate));
    }
    let bit_rate = track
        .details
        .get("bit_rate")
        .and_then(number)
        .or_else(|| tag(track, "BPS").and_then(|v| v.parse::<f64>().ok()));
    if let Some(bit_rate) = bit_rate {
        value.insert("bitRate".into(), json!(bit_rate.round() as u64));
    }
    if let Some(duration) = track.details.get("duration").and_then(number) {
        value.insert("durationSeconds".into(), json!(duration));
    }
    value.insert("language".into(), json!(tag(track, "language")));
    value.insert("title".into(), json!(tag(track, "title")));
    let mut disposition = Map::new();
    if let Some(source) = track.details.get("disposition").and_then(Value::as_object) {
        for flag in DISPOSITIONS {
            if source.get(*flag).and_then(Value::as_i64) == Some(1) {
                disposition.insert(camel(flag), json!(true));
            }
        }
    }
    value.insert("disposition".into(), Value::Object(disposition));
    Value::Object(value)
}

fn tag(track: &media_node::probe::Track, name: &str) -> Option<String> {
    track
        .tags
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .and_then(|(_, value)| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn camel(field: &str) -> String {
    let mut out = String::with_capacity(field.len());
    let mut upper = false;
    for c in field.chars() {
        if c == '_' {
            upper = true;
        } else if upper {
            out.extend(c.to_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

fn number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
    .filter(|v| v.is_finite() && *v >= 0.0)
}

fn frame_rate(value: &Value) -> Option<f64> {
    let text = value.as_str()?;
    let (num, den) = text.split_once('/').unwrap_or((text, "1"));
    let (num, den) = (num.parse::<f64>().ok()?, den.parse::<f64>().ok()?);
    (num > 0.0 && den > 0.0).then(|| ((num / den) * 1000.0).round() / 1000.0)
}

fn modified_seconds(nanos: &str) -> Option<i64> {
    nanos
        .parse::<i128>()
        .ok()
        .and_then(|n| i64::try_from(n / 1_000_000_000).ok())
}

fn file_name(relative: &str) -> &str {
    relative.rsplit(['/', '\\']).next().unwrap_or(relative)
}

/// Joins the library root as configured on the Node with an inventoried relative
/// path, keeping the Node's own separator style (POSIX or Windows).
fn join_node_path(root: &str, relative: &str) -> String {
    let windows = root.contains('\\') && !root.contains('/');
    let separator = if windows { '\\' } else { '/' };
    let relative = relative.trim_start_matches(['/', '\\']);
    let relative = if windows {
        relative.replace('/', "\\")
    } else {
        relative.to_owned()
    };
    let root = root.trim_end_matches(['/', '\\']);
    if root.is_empty() {
        format!("{separator}{relative}")
    } else {
        format!("{root}{separator}{relative}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_paths_with_the_node_separator() {
        assert_eq!(
            join_node_path("/volume1/media/films/", "Film (2020)/film.mkv"),
            "/volume1/media/films/Film (2020)/film.mkv"
        );
        assert_eq!(
            join_node_path(r"D:\Media\Films", "Film (2020)/film.mkv"),
            r"D:\Media\Films\Film (2020)\film.mkv"
        );
        assert_eq!(join_node_path("/", "a.mkv"), "/a.mkv");
        assert_eq!(file_name("Film (2020)/film.mkv"), "film.mkv");
    }

    #[test]
    fn summaries_never_copy_probe_filename_or_unreviewed_fields() {
        let mut format = std::collections::BTreeMap::new();
        format.insert("filename".into(), json!("/secret/root/film.mkv"));
        format.insert("format_name".into(), json!("matroska,webm"));
        format.insert("bit_rate".into(), json!("4500123"));
        format.insert("duration".into(), json!("5400.5"));
        format.insert("native_opus_tracks_v1".into(), json!([1]));
        let summary = container_summary(&format);
        assert!(!summary.to_string().contains("secret"));
        assert_eq!(summary["bitRate"], 4_500_123);
        assert_eq!(summary["durationSeconds"], 5400.5);

        let track: media_node::probe::Track = serde_json::from_value(json!({
            "index": 1, "codec_type": "audio", "codec_name": "opus", "channels": 6,
            "channel_layout": "5.1", "extradata": "private", "avg_frame_rate": "0/0",
            "tags": {"LANGUAGE": "fra", "title": "VF", "BPS": "256000", "ENCODER": "x"},
            "disposition": {"default": 1, "forced": 0, "comment": 1}
        }))
        .unwrap();
        let summary = track_summary(&track);
        assert_eq!(summary["language"], "fra");
        assert_eq!(summary["title"], "VF");
        assert_eq!(summary["bitRate"], 256_000);
        assert_eq!(summary["channelLayout"], "5.1");
        assert_eq!(
            summary["disposition"],
            json!({"default": true, "comment": true})
        );
        assert!(summary.get("frameRate").is_none());
        assert!(!summary.to_string().contains("private"));
        assert!(!summary.to_string().contains("ENCODER"));
        assert_eq!(
            frame_rate(&json!("24000/1001")),
            Some(23.976),
            "NTSC film rate"
        );
    }
}
