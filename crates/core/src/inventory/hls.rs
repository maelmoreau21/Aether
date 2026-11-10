//! HLS master playlist of an asset: one video variant and one audio rendition
//! per track a native player decodes, all cut and served by the Node
//! (`media_node::hls`). Relative URIs keep the capability path they came from.
use super::decode;
use crate::{
    database::query_scalar,
    identity::IdentityStore,
    media::{Probe, Track},
};
use axum::{
    body::Body,
    http::{Method, StatusCode, header},
    response::Response,
};
use std::fmt::Write;

/// Copied as-is into fMP4 and decoded natively by AVPlayer, ExoPlayer and Cast.
pub(crate) const VIDEO_CODECS: [&str; 3] = ["h264", "hevc", "av1"];
pub(crate) const AUDIO_CODECS: [&str; 7] = ["aac", "ac3", "eac3", "flac", "alac", "mp3", "opus"];
const DEFAULT_BANDWIDTH: u64 = 20_000_000;

fn number(track: &Track, key: &str) -> Option<i64> {
    let value = track.details.get(key)?;
    value.as_i64().or_else(|| value.as_str()?.parse().ok())
}
fn text<'a>(track: &'a Track, key: &str) -> Option<&'a str> {
    track.details.get(key)?.as_str()
}
fn flag(track: &Track, key: &str) -> bool {
    track
        .details
        .get("disposition")
        .and_then(|d| d.get(key))
        .and_then(serde_json::Value::as_i64)
        == Some(1)
}

/// RFC 6381 codec string, or `None` when the probe lacks what it needs.
pub(crate) fn video_codec(track: &Track) -> Option<String> {
    let level = number(track, "level").filter(|level| *level > 0);
    match track.codec_name.as_deref()? {
        "h264" => {
            let profile = match text(track, "profile")? {
                "Baseline" | "Constrained Baseline" => 0x42,
                "Main" => 0x4D,
                "High" => 0x64,
                "High 10" => 0x6E,
                _ => return None,
            };
            Some(format!("avc1.{profile:02X}00{:02X}", level?.min(255)))
        }
        "hevc" => {
            let (profile, compatibility) = match text(track, "profile")? {
                "Main" => (1, 6),
                "Main 10" => (2, 4),
                _ => return None,
            };
            Some(format!("hvc1.{profile}.{compatibility}.L{}.B0", level?))
        }
        "av1" => {
            let profile = match text(track, "profile") {
                Some("High") => 1,
                Some("Professional") => 2,
                _ => 0,
            };
            let depth = if text(track, "pix_fmt").is_some_and(|f| f.contains("10")) {
                10
            } else {
                8
            };
            Some(format!(
                "av01.{profile}.{:02}M.{depth:02}",
                level.unwrap_or(0).min(31)
            ))
        }
        _ => None,
    }
}
pub(crate) fn audio_codec(track: &Track) -> Option<&'static str> {
    Some(match track.codec_name.as_deref()? {
        "aac" if text(track, "profile") == Some("HE-AAC") => "mp4a.40.5",
        "aac" => "mp4a.40.2",
        "ac3" => "ac-3",
        "eac3" => "ec-3",
        "flac" => "fLaC",
        "alac" => "alac",
        "mp3" => "mp4a.40.34",
        "opus" => "Opus",
        _ => return None,
    })
}

fn is_video(track: &Track) -> bool {
    track.codec_type.as_deref() == Some("video")
        && !flag(track, "attached_pic")
        && track
            .codec_name
            .as_deref()
            .is_some_and(|c| VIDEO_CODECS.contains(&c))
}
fn is_audio(track: &Track) -> bool {
    track.codec_type.as_deref() == Some("audio")
        && track
            .codec_name
            .as_deref()
            .is_some_and(|c| AUDIO_CODECS.contains(&c))
}

/// An audio track native players cannot decode (DTS, TrueHD, PCM…): offered
/// as an AAC conversion when the administrator allows transcoding.
fn is_convertible_audio(track: &Track) -> bool {
    track.codec_type.as_deref() == Some("audio") && track.codec_name.is_some() && !is_audio(track)
}

/// Whether a Node HLS component names streams this asset can serve that way.
/// A conversion is only accepted for a track that needs one.
pub(crate) fn allowed(probe: &Probe, request: &media_node::hls::HlsRequest) -> bool {
    let has = |index: u32, test: fn(&Track) -> bool| {
        probe.streams.iter().any(|t| t.index == index && test(t))
    };
    let audio_ok: fn(&Track) -> bool = if request.convert {
        is_convertible_audio
    } else {
        is_audio
    };
    has(request.video, is_video) && request.audio.is_none_or(|audio| has(audio, audio_ok))
}

/// Quoted-string attribute: no quote, no control character, bounded length.
fn quoted(value: &str) -> String {
    value
        .chars()
        .filter(|c| *c != '"' && !c.is_control())
        .take(64)
        .collect()
}

/// `convert`: also offer AAC conversions of the tracks native players cannot decode.
pub(crate) fn master_playlist(probe: &Probe, convert: bool) -> Result<String, &'static str> {
    let video = probe
        .streams
        .iter()
        .find(|t| is_video(t))
        .ok_or("hls_video_unsupported")?;
    let audios: Vec<&Track> = probe
        .streams
        .iter()
        .filter(|t| is_audio(t) || (convert && is_convertible_audio(t)))
        .collect();
    let default = audios.iter().position(|t| flag(t, "default")).unwrap_or(0);
    let mut out = String::from("#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-INDEPENDENT-SEGMENTS\n");
    let mut names: Vec<String> = Vec::new();
    for (position, track) in audios.iter().enumerate() {
        let language = track
            .tags
            .get("language")
            .and_then(serde_json::Value::as_str)
            .filter(|l| !l.is_empty() && *l != "und");
        let title = track.tags.get("title").and_then(serde_json::Value::as_str);
        let mut name = quoted(
            &title
                .map(str::to_owned)
                .or_else(|| language.map(str::to_uppercase))
                .unwrap_or_else(|| format!("Audio {}", position + 1)),
        );
        // Names must be unique within a group.
        if names.contains(&name) {
            name = format!("{name} ({})", position + 1);
        }
        names.push(name.clone());
        let _ = write!(
            out,
            "#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"audio\",NAME=\"{name}\""
        );
        if let Some(language) = language {
            let _ = write!(out, ",LANGUAGE=\"{}\"", quoted(&language.to_lowercase()));
        }
        let yes = if position == default { "YES" } else { "NO" };
        let _ = write!(out, ",DEFAULT={yes},AUTOSELECT=YES");
        let converted = !is_audio(track);
        if let Some(channels) = number(track, "channels").filter(|c| *c > 0) {
            // The conversion keeps 5.1 at most (7.1 is folded down).
            let channels = if converted { channels.min(6) } else { channels };
            let _ = write!(out, ",CHANNELS=\"{channels}\"");
        }
        let kind = if converted { "aac" } else { "a" };
        let _ = writeln!(
            out,
            ",URI=\"hls-v{}-{kind}{}.m3u8\"",
            video.index, track.index
        );
    }
    let bandwidth = probe
        .format
        .get("bit_rate")
        .and_then(|v| v.as_str()?.parse::<u64>().ok())
        .or_else(|| number(video, "bit_rate").map(|b| b as u64))
        .map_or(DEFAULT_BANDWIDTH, |b| b.saturating_mul(6) / 5);
    let _ = write!(out, "#EXT-X-STREAM-INF:BANDWIDTH={bandwidth}");
    // CODECS lists every format of the variant, or is left out rather than guessed.
    let mut codecs = vec![video_codec(video)];
    for track in &audios {
        let codec = if is_audio(track) {
            audio_codec(track).map(str::to_owned)
        } else {
            Some("mp4a.40.2".to_owned())
        };
        if !codecs.contains(&codec) {
            codecs.push(codec);
        }
    }
    if let Some(codecs) = codecs.into_iter().collect::<Option<Vec<_>>>() {
        let _ = write!(out, ",CODECS=\"{}\"", codecs.join(","));
    }
    if let (Some(width), Some(height)) = (number(video, "width"), number(video, "height")) {
        let _ = write!(out, ",RESOLUTION={width}x{height}");
    }
    if !audios.is_empty() {
        out.push_str(",AUDIO=\"audio\"");
    }
    let _ = writeln!(out, "\nhls-v{}.m3u8", video.index);
    Ok(out)
}

pub async fn hls_master(store: &IdentityStore, asset_id: &str, method: &Method) -> Response {
    let probe: Result<Option<String>, _> = query_scalar("SELECT a.probe_json FROM inventory_assets a JOIN libraries l ON l.id=a.library_id WHERE a.id=? AND a.available=1 AND l.enabled=1")
        .bind(asset_id)
        .fetch_optional(&store.pool)
        .await;
    let probe = match probe {
        Ok(Some(json)) => decode(&json),
        Ok(None) => return crate::error(StatusCode::NOT_FOUND, "asset_unavailable"),
        Err(_) => return crate::error(StatusCode::INTERNAL_SERVER_ERROR, "inventory_unavailable"),
    };
    // AAC conversions spend Node CPU: they follow the transcoding setting.
    let convert = crate::server_settings::load(store)
        .await
        .is_ok_and(|settings| settings.transcoding_enabled);
    let text = match probe.as_ref().map(|probe| master_playlist(probe, convert)) {
        Ok(Ok(text)) => text,
        Ok(Err(code)) => return crate::error(StatusCode::UNPROCESSABLE_ENTITY, code),
        Err(_) => return crate::error(StatusCode::INTERNAL_SERVER_ERROR, "probe_invalid"),
    };
    let length = text.len();
    Response::builder()
        .header(header::CONTENT_TYPE, "application/vnd.apple.mpegurl")
        .header(header::CACHE_CONTROL, "private, no-cache")
        .header(header::CONTENT_LENGTH, length)
        .body(if method == Method::HEAD {
            Body::empty()
        } else {
            Body::from(text)
        })
        .unwrap_or_else(|_| crate::error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn track(value: serde_json::Value) -> Track {
        serde_json::from_value(value).unwrap()
    }
    fn probe(streams: Vec<serde_json::Value>) -> Probe {
        Probe {
            streams: streams.into_iter().map(track).collect(),
            format: [("bit_rate".to_owned(), json!("10000000"))].into(),
            chapters: Vec::new(),
        }
    }

    #[test]
    fn master_lists_playable_audio_renditions_and_exact_codecs() {
        let probe = probe(vec![
            json!({"index":0,"codec_type":"video","codec_name":"hevc","profile":"Main 10","level":150,"width":3840,"height":2160}),
            json!({"index":1,"codec_type":"audio","codec_name":"eac3","channels":6,"tags":{"language":"fre","title":"Français \"VFF\""},"disposition":{"default":0}}),
            json!({"index":2,"codec_type":"audio","codec_name":"aac","profile":"LC","channels":2,"tags":{"language":"eng"},"disposition":{"default":1}}),
            json!({"index":3,"codec_type":"audio","codec_name":"dts","channels":6}),
            json!({"index":4,"codec_type":"video","codec_name":"mjpeg","disposition":{"attached_pic":1}}),
        ]);
        // With transcoding allowed, the DTS track is offered as AAC 5.1.
        let converted = master_playlist(&probe, true).unwrap();
        assert!(
            converted.contains(r#"CHANNELS="6",URI="hls-v0-aac3.m3u8""#),
            "{converted}"
        );
        assert!(
            converted.contains(r#"CODECS="hvc1.2.4.L150.B0,ec-3,mp4a.40.2""#),
            "{converted}"
        );
        let text = master_playlist(&probe, false).unwrap();
        assert!(text.contains("NAME=\"Français VFF\",LANGUAGE=\"fre\",DEFAULT=NO,AUTOSELECT=YES,CHANNELS=\"6\",URI=\"hls-v0-a1.m3u8\""));
        assert!(text.contains("NAME=\"ENG\",LANGUAGE=\"eng\",DEFAULT=YES,AUTOSELECT=YES,CHANNELS=\"2\",URI=\"hls-v0-a2.m3u8\""));
        assert!(!text.contains("a3.m3u8"), "DTS is not offered: {text}");
        assert!(text.contains("#EXT-X-STREAM-INF:BANDWIDTH=12000000,CODECS=\"hvc1.2.4.L150.B0,ec-3,mp4a.40.2\",RESOLUTION=3840x2160,AUDIO=\"audio\"\nhls-v0.m3u8\n"));
    }

    #[test]
    fn codec_strings_and_refusals() {
        let h264 = track(
            json!({"index":0,"codec_type":"video","codec_name":"h264","profile":"High","level":40}),
        );
        assert_eq!(video_codec(&h264).as_deref(), Some("avc1.640028"));
        let av1 = track(
            json!({"index":0,"codec_type":"video","codec_name":"av1","profile":"Main","level":8,"pix_fmt":"yuv420p10le"}),
        );
        assert_eq!(video_codec(&av1).as_deref(), Some("av01.0.08M.10"));
        // An unknown profile leaves CODECS out instead of guessing.
        let odd = probe(vec![
            json!({"index":0,"codec_type":"video","codec_name":"h264","profile":"High 4:4:4"}),
        ]);
        let text = master_playlist(&odd, true).unwrap();
        assert!(
            !text.contains("CODECS") && !text.contains("AUDIO="),
            "{text}"
        );
        let mpeg2 = probe(vec![
            json!({"index":0,"codec_type":"video","codec_name":"mpeg2video"}),
        ]);
        assert_eq!(master_playlist(&mpeg2, true), Err("hls_video_unsupported"));
        let request = media_node::hls::request("hls-v0-a3.m3u8").unwrap();
        let with_dts = probe(vec![
            json!({"index":0,"codec_type":"video","codec_name":"h264"}),
            json!({"index":3,"codec_type":"audio","codec_name":"dts"}),
        ]);
        assert!(!allowed(&with_dts, &request));
        let conversion = media_node::hls::request("hls-v0-aac3.m3u8").unwrap();
        assert!(allowed(&with_dts, &conversion));
        let of_video = media_node::hls::request("hls-v0-aac0.m3u8").unwrap();
        assert!(
            !allowed(&with_dts, &of_video),
            "only audio that needs it is converted"
        );
        assert!(allowed(
            &with_dts,
            &media_node::hls::request("hls-v0.m3u8").unwrap()
        ));
    }
}
