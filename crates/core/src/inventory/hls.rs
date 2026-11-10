//! HLS master playlist of an asset: one video variant and one audio rendition
//! per track a native player decodes, all cut and served by the Node
//! (`media_node::hls`). Relative URIs keep the capability path they came from.
use super::decode;
use crate::{
    database::query,
    identity::IdentityStore,
    media::{Probe, Track},
};
use axum::{
    body::Body,
    http::{Method, StatusCode, header},
    response::Response,
};
use sqlx::Row;
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
/// A conversion is only accepted for a track that needs one, a reduced
/// rendition only when the master playlist offers it for this video.
pub(crate) fn allowed(probe: &Probe, request: &media_node::hls::HlsRequest) -> bool {
    let has = |index: u32, test: fn(&Track) -> bool| {
        probe.streams.iter().any(|t| t.index == index && test(t))
    };
    let audio_ok: fn(&Track) -> bool = if request.convert {
        is_convertible_audio
    } else {
        is_audio
    };
    let encode_ok = request.encode.is_none_or(|encode| {
        probe
            .streams
            .iter()
            .find(|t| t.index == request.video && is_video(t))
            .is_some_and(|video| {
                let audios = renditions_audio(probe, true);
                reduced(probe, video, &audios)
                    .iter()
                    .any(|offered| offered.encode == encode)
            })
    });
    has(request.video, is_video)
        && request.audio.is_none_or(|audio| has(audio, audio_ok))
        && encode_ok
}

/// Audio renditions of the master playlist, in order.
fn renditions_audio(probe: &Probe, convert: bool) -> Vec<&Track> {
    probe
        .streams
        .iter()
        .filter(|t| is_audio(t) || (convert && is_convertible_audio(t)))
        .collect()
}

/// Peak bitrate of the original variant: the whole file, with a margin.
fn original_bandwidth(probe: &Probe, video: &Track) -> u64 {
    probe
        .format
        .get("bit_rate")
        .and_then(|v| v.as_str()?.parse::<u64>().ok())
        .or_else(|| number(video, "bit_rate").map(|b| b as u64))
        .map_or(DEFAULT_BANDWIDTH, |b| b.saturating_mul(6) / 5)
}

/// The heaviest audio rendition a reduced variant may be played with.
fn audio_peak(audios: &[&Track]) -> u64 {
    audios
        .iter()
        .map(|track| {
            if !is_audio(track) {
                return 320_000; // the Node's AAC conversion
            }
            number(track, "bit_rate")
                .or_else(|| {
                    let tags = &track.tags;
                    let bps = tags.get("BPS").or_else(|| tags.get("BPS-eng"))?;
                    bps.as_str()?.parse().ok()
                })
                .filter(|b| *b > 0)
                .map_or(640_000, |b| b as u64)
        })
        .max()
        .unwrap_or(0)
}

/// A reduced H.264 variant offered for a video stream.
#[derive(Debug, PartialEq, Eq)]
struct Reduced {
    encode: media_node::hls::Encode,
    width: u32,
    height: u32,
    bandwidth: u64,
}

/// Reduced variants for `video`: only for a picture larger than the
/// rendition's box, lighter than the original, and of a dynamic range the
/// Node turns into SDR (Dolby Vision profile 5 has no HDR10 base to tone map).
fn reduced(probe: &Probe, video: &Track, audios: &[&Track]) -> Vec<Reduced> {
    let range = super::resources::video_dynamic_range(video);
    if range.as_deref() == Some("dolby_vision_5") {
        return Vec::new();
    }
    let (Some(width), Some(height)) = (number(video, "width"), number(video, "height")) else {
        return Vec::new();
    };
    let (Ok(width), Ok(height)) = (u32::try_from(width), u32::try_from(height)) else {
        return Vec::new();
    };
    let original = original_bandwidth(probe, video);
    let audio = audio_peak(audios);
    media_node::hls::RENDITIONS
        .into_iter()
        .filter(|r| width > r.width || height > r.height)
        .filter_map(|rendition| {
            let (w, h) = media_node::hls::renditions::fit(width, height, rendition)?;
            // Rate control peaks at `maxrate`; the same margin as the original.
            let bandwidth = u64::from(rendition.kbps) * 1000 * 6 / 5 + audio;
            (bandwidth < original).then_some(Reduced {
                encode: media_node::hls::Encode {
                    height: rendition.height,
                    tonemap: range.is_some(),
                },
                width: w,
                height: h,
                bandwidth,
            })
        })
        .collect()
}

/// Quoted-string attribute: no quote, no control character, bounded length.
fn quoted(value: &str) -> String {
    value
        .chars()
        .filter(|c| *c != '"' && !c.is_control())
        .take(64)
        .collect()
}

/// Embedded subtitle formats the Node extracts to WebVTT.
const TEXT_SUBTITLE_CODECS: [&str; 5] = ["subrip", "webvtt", "ass", "ssa", "mov_text"];

/// A WebVTT rendition: `hls-s{index}` for an embedded text track, `hls-x{n}`
/// for a sidecar file. Image subtitles (PGS, DVD) have no HLS form.
#[derive(Debug, PartialEq)]
pub(crate) struct SubtitleRendition {
    pub stem: String,
    pub name: String,
    pub language: Option<String>,
    pub forced: bool,
}

/// `allowed`: the library policy on embedded subtitles, by codec.
pub(crate) fn subtitle_renditions(
    probe: &Probe,
    allowed: impl Fn(Option<&str>) -> bool,
    sidecars: &[media_contracts::inventory::ExternalSubtitle],
) -> Vec<SubtitleRendition> {
    let mut out: Vec<SubtitleRendition> = probe
        .streams
        .iter()
        .filter(|t| {
            t.codec_type.as_deref() == Some("subtitle")
                && t.codec_name
                    .as_deref()
                    .is_some_and(|c| TEXT_SUBTITLE_CODECS.contains(&c))
                && allowed(t.codec_name.as_deref())
        })
        .map(|t| {
            let language = t
                .tags
                .get("language")
                .and_then(serde_json::Value::as_str)
                .filter(|l| !l.is_empty() && *l != "und")
                .map(str::to_lowercase);
            let title = t.tags.get("title").and_then(serde_json::Value::as_str);
            SubtitleRendition {
                stem: format!("hls-s{}", t.index),
                name: title
                    .map(str::to_owned)
                    .or_else(|| language.as_deref().map(str::to_uppercase))
                    .unwrap_or_else(|| format!("Sous-titres {}", t.index)),
                language,
                forced: flag(t, "forced"),
            }
        })
        .collect();
    out.extend(sidecars.iter().enumerate().map(|(n, s)| SubtitleRendition {
        stem: format!("hls-x{n}"),
        name: s.label.clone(),
        language: Some(s.language.to_lowercase()).filter(|l| !l.is_empty() && l != "und"),
        forced: s.forced,
    }));
    out
}

/// A subtitle component of the HLS path: (embedded?, index, playlist?).
pub fn subtitle_component(component: &str) -> Option<(bool, usize, bool)> {
    let (embedded, rest) = if let Some(rest) = component.strip_prefix("hls-s") {
        (true, rest)
    } else {
        (false, component.strip_prefix("hls-x")?)
    };
    let (index, playlist) = if let Some(index) = rest.strip_suffix(".m3u8") {
        (index, true)
    } else {
        (rest.strip_suffix(".vtt")?, false)
    };
    // Digits only: no sign, no leading `+`.
    if index.is_empty() || !index.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((embedded, index.parse().ok()?, playlist))
}

/// One segment holding the whole WebVTT file: players fetch it once.
pub(crate) fn subtitle_playlist(stem: &str, duration: f64) -> String {
    let duration = if duration.is_finite() && duration > 0.0 {
        duration
    } else {
        86_400.0
    };
    format!(
        "#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-TARGETDURATION:{}\n#EXT-X-MEDIA-SEQUENCE:0\n#EXT-X-PLAYLIST-TYPE:VOD\n#EXTINF:{duration:.3},\n{stem}.vtt\n#EXT-X-ENDLIST\n",
        duration.ceil() as u64
    )
}

/// `transcode` (the administrator allows encoding): also offer AAC conversions
/// of the tracks native players cannot decode, and reduced H.264 variants
/// after the original one.
pub(crate) fn master_playlist(
    probe: &Probe,
    transcode: bool,
    subtitles: &[SubtitleRendition],
) -> Result<String, &'static str> {
    let video = probe
        .streams
        .iter()
        .find(|t| is_video(t))
        .ok_or("hls_video_unsupported")?;
    let audios = renditions_audio(probe, transcode);
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
    let mut names: Vec<String> = Vec::new();
    for (position, subtitle) in subtitles.iter().enumerate() {
        let mut name = quoted(&subtitle.name);
        if name.is_empty() || names.contains(&name) {
            name = format!("{name} ({})", position + 1);
        }
        names.push(name.clone());
        let _ = write!(
            out,
            "#EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID=\"subs\",NAME=\"{name}\""
        );
        if let Some(language) = &subtitle.language {
            let _ = write!(out, ",LANGUAGE=\"{}\"", quoted(language));
        }
        // Forced tracks follow the audio language on their own; others wait for the user.
        let (autoselect, forced) = if subtitle.forced {
            ("YES", "YES")
        } else {
            ("NO", "NO")
        };
        let _ = writeln!(
            out,
            ",DEFAULT=NO,AUTOSELECT={autoselect},FORCED={forced},URI=\"{}.m3u8\"",
            subtitle.stem
        );
    }
    // CODECS lists every format of the variant, or is left out rather than guessed.
    let mut audio_codecs = Vec::new();
    for track in &audios {
        let codec = if is_audio(track) {
            audio_codec(track).map(str::to_owned)
        } else {
            Some("mp4a.40.2".to_owned())
        };
        if !audio_codecs.contains(&codec) {
            audio_codecs.push(codec);
        }
    }
    let variant = |out: &mut String,
                   bandwidth: u64,
                   video_codec: Option<String>,
                   size: Option<(i64, i64)>,
                   uri: &str| {
        let _ = write!(out, "#EXT-X-STREAM-INF:BANDWIDTH={bandwidth}");
        let codecs = std::iter::once(video_codec).chain(audio_codecs.iter().cloned());
        if let Some(codecs) = codecs.collect::<Option<Vec<_>>>() {
            let _ = write!(out, ",CODECS=\"{}\"", codecs.join(","));
        }
        if let Some((width, height)) = size {
            let _ = write!(out, ",RESOLUTION={width}x{height}");
        }
        if !audios.is_empty() {
            out.push_str(",AUDIO=\"audio\"");
        }
        if !subtitles.is_empty() {
            out.push_str(",SUBTITLES=\"subs\"");
        }
        let _ = writeln!(out, "\n{uri}");
    };
    // The original first: players start on the first variant listed.
    variant(
        &mut out,
        original_bandwidth(probe, video),
        video_codec(video),
        number(video, "width").zip(number(video, "height")),
        &format!("hls-v{}.m3u8", video.index),
    );
    if transcode {
        for offered in reduced(probe, video, &audios) {
            let request = media_node::hls::HlsRequest {
                video: video.index,
                audio: None,
                convert: false,
                encode: Some(offered.encode),
                resource: media_node::hls::service::Resource::Playlist,
            };
            variant(
                &mut out,
                offered.bandwidth,
                Some(media_node::transcode::h264_codec(offered.encode.height).to_owned()),
                Some((i64::from(offered.width), i64::from(offered.height))),
                &request.component(),
            );
        }
    }
    Ok(out)
}

/// Probe, then subtitle renditions the library policy allows.
async fn load(
    store: &IdentityStore,
    asset_id: &str,
) -> Result<(Probe, Vec<SubtitleRendition>), Response> {
    let row = query("SELECT a.probe_json,a.library_id,t.subtitles_json FROM inventory_assets a JOIN libraries l ON l.id=a.library_id LEFT JOIN inventory_subtitles t ON t.asset_id=a.id WHERE a.id=? AND a.available=1 AND l.enabled=1")
        .bind(asset_id)
        .fetch_optional(&store.pool)
        .await;
    let row = match row {
        Ok(Some(row)) => row,
        Ok(None) => return Err(crate::error(StatusCode::NOT_FOUND, "asset_unavailable")),
        Err(_) => {
            return Err(crate::error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "inventory_unavailable",
            ));
        }
    };
    let probe = decode(row.get("probe_json"))
        .map_err(|_| crate::error(StatusCode::INTERNAL_SERVER_ERROR, "probe_invalid"))?;
    let options = super::playback::playback_library_options(
        store,
        row.get::<String, _>("library_id").as_str(),
    )
    .await
    .map_err(|_| {
        crate::error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "library_options_unavailable",
        )
    })?;
    let sidecars: Vec<media_contracts::inventory::ExternalSubtitle> = row
        .get::<Option<String>, _>("subtitles_json")
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default();
    let subtitles = subtitle_renditions(
        &probe,
        |codec| options.embedded_subtitle_allowed(codec),
        &sidecars,
    );
    Ok((probe, subtitles))
}

fn playlist_response(text: String, method: &Method) -> Response {
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

/// Media playlist of one subtitle rendition, or `None` when it is not offered.
pub async fn hls_subtitle_playlist(
    store: &IdentityStore,
    asset_id: &str,
    stem: &str,
    method: &Method,
) -> Response {
    let (probe, subtitles) = match load(store, asset_id).await {
        Ok(loaded) => loaded,
        Err(response) => return response,
    };
    if !subtitles.iter().any(|s| s.stem == stem) {
        return crate::error(StatusCode::NOT_FOUND, "unknown_subtitle");
    }
    let duration = probe
        .format
        .get("duration")
        .and_then(|v| v.as_str()?.parse::<f64>().ok())
        .unwrap_or(0.0);
    playlist_response(subtitle_playlist(stem, duration), method)
}

pub async fn hls_master(store: &IdentityStore, asset_id: &str, method: &Method) -> Response {
    let (probe, subtitles) = match load(store, asset_id).await {
        Ok(loaded) => loaded,
        Err(response) => return response,
    };
    // AAC conversions and reduced variants spend Node CPU: they follow the transcoding setting.
    let transcode = crate::server_settings::load(store)
        .await
        .is_ok_and(|settings| settings.transcoding_enabled);
    match master_playlist(&probe, transcode, &subtitles) {
        Ok(text) => playlist_response(text, method),
        Err(code) => crate::error(StatusCode::UNPROCESSABLE_ENTITY, code),
    }
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
        let converted = master_playlist(&probe, true, &[]).unwrap();
        assert!(
            converted.contains(r#"CHANNELS="6",URI="hls-v0-aac3.m3u8""#),
            "{converted}"
        );
        assert!(
            converted.contains(r#"CODECS="hvc1.2.4.L150.B0,ec-3,mp4a.40.2""#),
            "{converted}"
        );
        let text = master_playlist(&probe, false, &[]).unwrap();
        assert!(text.contains("NAME=\"Français VFF\",LANGUAGE=\"fre\",DEFAULT=NO,AUTOSELECT=YES,CHANNELS=\"6\",URI=\"hls-v0-a1.m3u8\""));
        assert!(text.contains("NAME=\"ENG\",LANGUAGE=\"eng\",DEFAULT=YES,AUTOSELECT=YES,CHANNELS=\"2\",URI=\"hls-v0-a2.m3u8\""));
        assert!(!text.contains("a3.m3u8"), "DTS is not offered: {text}");
        assert!(text.contains("#EXT-X-STREAM-INF:BANDWIDTH=12000000,CODECS=\"hvc1.2.4.L150.B0,ec-3,mp4a.40.2\",RESOLUTION=3840x2160,AUDIO=\"audio\"\nhls-v0.m3u8\n"));
    }

    #[test]
    fn subtitles_become_webvtt_renditions() {
        let probe = probe(vec![
            json!({"index":0,"codec_type":"video","codec_name":"h264"}),
            json!({"index":1,"codec_type":"subtitle","codec_name":"subrip","tags":{"language":"FRE"},"disposition":{"forced":1}}),
            json!({"index":2,"codec_type":"subtitle","codec_name":"hdmv_pgs_subtitle"}),
            json!({"index":3,"codec_type":"subtitle","codec_name":"ass","tags":{"title":"Signs"}}),
        ]);
        let sidecar = media_contracts::inventory::ExternalSubtitle {
            language: "en".into(),
            label: "English".into(),
            forced: false,
            sdh: false,
            source_format: "srt".into(),
            webvtt: "WEBVTT\n".into(),
            ass: None,
        };
        // The library refuses ASS: only SubRip and the sidecar remain; PGS has no HLS form.
        let subs = subtitle_renditions(&probe, |c| c != Some("ass"), &[sidecar]);
        let stems: Vec<&str> = subs.iter().map(|s| s.stem.as_str()).collect();
        assert_eq!(stems, ["hls-s1", "hls-x0"]);
        let text = master_playlist(&probe, false, &subs).unwrap();
        assert!(text.contains("#EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID=\"subs\",NAME=\"FRE\",LANGUAGE=\"fre\",DEFAULT=NO,AUTOSELECT=YES,FORCED=YES,URI=\"hls-s1.m3u8\"\n"), "{text}");
        assert!(text.contains("NAME=\"English\",LANGUAGE=\"en\",DEFAULT=NO,AUTOSELECT=NO,FORCED=NO,URI=\"hls-x0.m3u8\""), "{text}");
        assert!(
            text.contains(",SUBTITLES=\"subs\"\nhls-v0.m3u8\n"),
            "{text}"
        );
        assert_eq!(subtitle_component("hls-s12.m3u8"), Some((true, 12, true)));
        assert_eq!(subtitle_component("hls-x0.vtt"), Some((false, 0, false)));
        assert_eq!(subtitle_component("hls-s+1.vtt"), None);
        assert_eq!(subtitle_component("hls-v0.m3u8"), None);
        let media = subtitle_playlist("hls-s1", 5400.25);
        assert!(media.contains("#EXT-X-TARGETDURATION:5401\n"), "{media}");
        assert!(
            media.contains("#EXTINF:5400.250,\nhls-s1.vtt\n#EXT-X-ENDLIST\n"),
            "{media}"
        );
    }

    #[test]
    fn reduced_variants_follow_the_original_below_its_size() {
        let uhd = probe(vec![
            json!({"index":0,"codec_type":"video","codec_name":"hevc","profile":"Main 10","level":153,"width":3840,"height":1600}),
            json!({"index":1,"codec_type":"audio","codec_name":"eac3","channels":6,"tags":{"BPS":"768000"}}),
            json!({"index":2,"codec_type":"audio","codec_name":"truehd","channels":8}),
        ]);
        let text = master_playlist(&uhd, true, &[]).unwrap();
        let expected = "#EXT-X-STREAM-INF:BANDWIDTH=12000000,CODECS=\"hvc1.2.4.L153.B0,ec-3,mp4a.40.2\",RESOLUTION=3840x1600,AUDIO=\"audio\"\nhls-v0.m3u8\n\
             #EXT-X-STREAM-INF:BANDWIDTH=10368000,CODECS=\"avc1.640029,ec-3,mp4a.40.2\",RESOLUTION=1920x800,AUDIO=\"audio\"\nhls-v0-h1080.m3u8\n\
             #EXT-X-STREAM-INF:BANDWIDTH=5568000,CODECS=\"avc1.640029,ec-3,mp4a.40.2\",RESOLUTION=1280x534,AUDIO=\"audio\"\nhls-v0-h720.m3u8\n";
        assert!(text.ends_with(expected), "{text}");
        let offered =
            |probe: &Probe, raw: &str| allowed(probe, &media_node::hls::request(raw).unwrap());
        assert!(offered(&uhd, "hls-v0-h720-3.m4s"));
        assert!(
            !offered(&uhd, "hls-v0-t720-3.m4s"),
            "SDR is not tone mapped"
        );
        // Encoding is the administrator's call.
        let plain = master_playlist(&uhd, false, &[]).unwrap();
        assert!(!plain.contains("h1080") && plain.ends_with("hls-v0.m3u8\n"));
        // HDR10 is tone mapped; Dolby Vision 5 has nothing to map from.
        let mut hdr = uhd.clone();
        hdr.streams[0]
            .details
            .insert("color_transfer".into(), json!("smpte2084"));
        let text = master_playlist(&hdr, true, &[]).unwrap();
        assert!(text.contains("hls-v0-t1080.m3u8\n") && !text.contains("-h1080"));
        assert!(offered(&hdr, "hls-v0-t1080-init.mp4") && !offered(&hdr, "hls-v0-h1080.m3u8"));
        let mut dovi = uhd.clone();
        dovi.streams[0].details.insert(
            "side_data_list".into(),
            json!([{"side_data_type":"DOVI configuration record","dv_profile":5}]),
        );
        assert!(
            !master_playlist(&dovi, true, &[])
                .unwrap()
                .contains("-t1080")
        );
        assert!(!offered(&dovi, "hls-v0-t720.m3u8"));
        // A 1080p source gets 720p only; a light one nothing at all.
        let full_hd = probe(vec![
            json!({"index":0,"codec_type":"video","codec_name":"h264","profile":"High","level":41,"width":1920,"height":1080}),
        ]);
        let text = master_playlist(&full_hd, true, &[]).unwrap();
        assert!(
            !text.contains("h1080") && text.contains("RESOLUTION=1280x720\nhls-v0-h720.m3u8\n"),
            "{text}"
        );
        assert!(!offered(&full_hd, "hls-v0-h1080.m3u8"));
        let mut light = full_hd.clone();
        light.format.insert("bit_rate".into(), json!("3000000"));
        assert!(!master_playlist(&light, true, &[]).unwrap().contains("h720"));
        assert!(!offered(&light, "hls-v0-h720.m3u8"));
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
        let text = master_playlist(&odd, true, &[]).unwrap();
        assert!(
            !text.contains("CODECS") && !text.contains("AUDIO="),
            "{text}"
        );
        let mpeg2 = probe(vec![
            json!({"index":0,"codec_type":"video","codec_name":"mpeg2video"}),
        ]);
        assert_eq!(
            master_playlist(&mpeg2, true, &[]),
            Err("hls_video_unsupported")
        );
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
