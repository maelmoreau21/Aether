//! HLS with packet copy for native players (AVPlayer, ExoPlayer, AirPlay,
//! Chromecast): a VOD playlist cut on the source's own keyframes, read from the
//! container index, and segments produced independently on request.
//!
//! - `keyframes`: Matroska Cues and MP4 sample tables, without reading the payload.
//! - `playlist`: segment boundaries and the M3U8 text.
//! - `renditions`: reduced H.264 renditions, re-encoded on the same cuts.
//! - `segments`: initialization/media split of FFmpeg's fragmented output.
//! - `service`: component names and the Node endpoint behind them.

pub mod keyframes;
pub mod playlist;
pub mod renditions;
pub mod segments;
pub mod service;

pub use keyframes::Keyframes;
pub use playlist::Segment;
pub use renditions::{RENDITIONS, Rendition};
pub use service::{Encode, HlsRequest, HlsStreams, request};

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs::File, io::ErrorKind, path::PathBuf};

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/generated")
            .join(name)
    }
    fn read(name: &str, video: u32) -> std::io::Result<Keyframes> {
        keyframes::read(&mut File::open(fixture(name))?, video)
    }

    #[test]
    fn matroska_cues_and_mp4_sample_tables_agree_on_the_same_remux() {
        for (mkv, mp4) in [
            ("av1-opus.mkv", "av1-opus.mp4"),
            (
                "representative/representative.mkv",
                "representative/representative.mp4",
            ),
            ("p1/clip-mkv-en.mkv", "p1/clip-mp4-en.mp4"),
        ] {
            if !fixture(mkv).exists() {
                eprintln!("skipping: generated fixtures missing ({mkv})");
                return;
            }
            let (from_cues, from_tables) = (read(mkv, 0).unwrap(), read(mp4, 0).unwrap());
            assert!(from_cues.times_ms[0] < 100, "{mkv} starts late");
            assert_eq!(
                from_cues.times_ms.len(),
                from_tables.times_ms.len(),
                "{mkv} vs {mp4}"
            );
            // The MP4 remux may shift the whole timeline a few ms (ffprobe agrees):
            // the spacing between keyframes must still match to the millisecond.
            let shift = from_tables.times_ms[0] as i64 - from_cues.times_ms[0] as i64;
            assert!(shift.abs() < 50, "{mkv} vs {mp4} shifted by {shift} ms");
            for (a, b) in from_cues.times_ms.iter().zip(&from_tables.times_ms) {
                assert!(
                    (*b as i64 - *a as i64 - shift).abs() <= 1,
                    "{mkv} {a} vs {mp4} {b}"
                );
            }
            assert!(from_cues.duration_ms.abs_diff(from_tables.duration_ms) <= 50);
            let cut = playlist::segments(&from_cues, playlist::TARGET_MS);
            assert_eq!(cut.last().unwrap().end_ms, from_cues.duration_ms);
        }
    }

    async fn hashes(path: &std::path::Path, stream: &str) -> Vec<String> {
        let output = tokio::process::Command::new("ffprobe")
            .args(["-v", "error", "-select_streams", stream, "-show_packets"])
            .args([
                "-show_data_hash",
                "sha256",
                "-show_entries",
                "packet=data_hash",
            ])
            .args(["-of", "csv=p=0"])
            .arg(path)
            .output()
            .await
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    /// Serves a rendition's playlist, initialization section and every segment
    /// (out of order) and writes them joined into `out`.
    async fn rebuild(
        root: &std::path::Path,
        name: &str,
        profile: &str,
        out: &std::path::Path,
    ) -> usize {
        use http::Method;
        use http_body_util::BodyExt;
        let streams = HlsStreams::default();
        let transcodes =
            crate::transcode::Transcodes::new(crate::transcode::TranscodeConfig::default());
        let transcodes = &transcodes;
        let mut joined = Vec::new();
        let get = |component: &str| {
            let request = request(component).unwrap();
            let streams = &streams;
            async move {
                let opened = crate::OpenedFile::open_in_root(root, std::path::Path::new(name))
                    .await
                    .unwrap();
                let response = streams
                    .serve(opened, "fixture".into(), request, Method::GET, transcodes)
                    .await
                    .unwrap();
                assert_eq!(response.status(), http::StatusCode::OK);
                response.into_body().collect().await.unwrap().to_bytes()
            }
        };
        let playlist = String::from_utf8(get(&format!("{profile}.m3u8")).await.to_vec()).unwrap();
        let count = playlist.lines().filter(|l| l.ends_with(".m4s")).count();
        assert!(count >= 2, "{playlist}");
        joined.extend_from_slice(&get(&format!("{profile}-init.mp4")).await);
        // Out of order on purpose: each segment must stand on its own.
        let mut order: Vec<usize> = (0..count).rev().collect();
        order.sort_by_key(|i| (i % 2, *i));
        let mut parts = vec![Vec::new(); count];
        for i in order {
            parts[i] = get(&format!("{profile}-{i}.m4s")).await.to_vec();
            assert_ne!(
                &parts[i][4..8],
                b"ftyp",
                "segment {i} carries an init section"
            );
        }
        // Also as served, beside `out`, for players reading the playlist itself.
        let dir = out.parent().unwrap();
        std::fs::write(dir.join(format!("{profile}.m3u8")), &playlist).unwrap();
        std::fs::write(dir.join(format!("{profile}-init.mp4")), &joined).unwrap();
        for (i, part) in parts.into_iter().enumerate() {
            std::fs::write(dir.join(format!("{profile}-{i}.m4s")), &part).unwrap();
            joined.extend_from_slice(&part);
        }
        std::fs::write(out, &joined).unwrap();
        count
    }

    async fn packet_times(path: &std::path::Path) -> Vec<String> {
        let out = tokio::process::Command::new("ffprobe")
            .args(["-v", "error", "-select_streams", "v:0", "-show_entries"])
            .args(["packet=pts_time,dts_time,flags", "-of", "csv=p=0"])
            .arg(path)
            .output()
            .await
            .unwrap();
        String::from_utf8(out.stdout)
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    /// Video rendition: every source video packet exactly once, in order.
    /// Audio rendition: one unbroken run of source packets, nothing lost or
    /// repeated at the joins; only the ends may differ (a priming packet before
    /// the first keyframe, the final packet rewritten by the end trim).
    async fn rebuild_matches_source(root: PathBuf, name: &str, audio_stream: u32) {
        let temp = tempfile::tempdir().unwrap();
        let original = root.join(name);
        let video_out = temp.path().join("video.mp4");
        rebuild(&root, name, "hls-v0", &video_out).await;
        let (video, source_video) = (
            hashes(&video_out, "v:0").await,
            hashes(&original, "v:0").await,
        );
        if video != source_video {
            let first = video.iter().zip(&source_video).position(|(a, b)| a != b);
            let at = first
                .unwrap_or(video.len().min(source_video.len()))
                .saturating_sub(2);
            let (got, want) = (
                packet_times(&video_out).await,
                packet_times(&original).await,
            );
            panic!(
                "{name}: {} vs {} video packets, first difference at {first:?}\nrebuilt {:?}\nsource  {:?}",
                video.len(),
                source_video.len(),
                &got[at.min(got.len())..(at + 6).min(got.len())],
                &want[at.min(want.len())..(at + 6).min(want.len())],
            );
        }
        let audio_out = temp.path().join("audio.mp4");
        rebuild(&root, name, &format!("hls-v0-a{audio_stream}"), &audio_out).await;
        let (audio, source_audio) = (
            hashes(&audio_out, "a:0").await,
            hashes(&original, "a:0").await,
        );
        let inner = &audio[..audio.len() - 1];
        let start = source_audio
            .iter()
            .position(|packet| *packet == inner[0])
            .unwrap();
        assert!(start <= 1, "{name}: audio starts {start} packets late");
        assert_eq!(
            inner,
            &source_audio[start..start + inner.len()],
            "{name}: audio joins"
        );
        assert!(source_audio.len() - (start + inner.len()) <= 1);
    }

    /// Needs FFmpeg ≥ 6 (`-fd`), as shipped in the Node image.
    #[tokio::test]
    #[ignore = "requires the Node image's FFmpeg"]
    async fn independent_segments_rebuild_every_video_packet_exactly_once() {
        rebuild_matches_source(fixture("representative"), "representative.mkv", 1).await;
    }

    /// Real GOPs: H.264 with B-frames and a keyframe every 2 s, AAC audio, in
    /// both containers, so cuts fall between keyframes as in a real library.
    #[tokio::test]
    #[ignore = "requires the Node image's FFmpeg (libx264)"]
    async fn h264_gops_with_b_frames_are_cut_cleanly_in_mkv_and_mp4() {
        let temp = tempfile::tempdir().unwrap();
        for name in ["gop.mkv", "gop.mp4"] {
            // The Node build has no lavfi sources: re-encode the AV1 fixture.
            let status = tokio::process::Command::new("ffmpeg")
                .args(["-v", "error", "-i"])
                .arg(fixture("representative/representative.mkv"))
                .args(["-map", "0:v:0", "-map", "0:a:0", "-c:v", "libx264"])
                .args(["-preset", "veryfast", "-bf", "3"])
                .args(["-force_key_frames", "expr:gte(t,n_forced*2)"])
                .args(["-x264-params", "keyint=1000:min-keyint=1000:scenecut=0"])
                .args(["-c:a", "aac", "-b:a", "96k"])
                .arg(temp.path().join(name))
                .status()
                .await
                .unwrap();
            assert!(status.success());
            let frames = read(temp.path().join(name).to_str().unwrap(), 0).unwrap();
            assert!(frames.times_ms.len() >= 5, "{name}: {:?}", frames.times_ms);
            for pair in frames.times_ms.windows(2) {
                assert!(pair[1] - pair[0] >= 1_900, "{name}: {:?}", frames.times_ms);
            }
            rebuild_matches_source(temp.path().to_owned(), name, 1).await;
        }
    }

    /// Reduced rendition: segments encoded apart, out of order, join into one
    /// stream showing every source frame exactly once at its source time, each
    /// segment opening on a keyframe, decode times strictly increasing across
    /// the joins, at the size `renditions::fit` announces to the master playlist.
    #[tokio::test]
    #[ignore = "requires the Node image's FFmpeg (libx264)"]
    async fn reduced_rendition_segments_join_into_the_source_frames() {
        let temp = tempfile::tempdir().unwrap();
        let name = "scope.mkv";
        // Scope picture with B-frames and a keyframe every 2 s (no lavfi in the Node build).
        let status = tokio::process::Command::new("ffmpeg")
            .args(["-v", "error", "-i"])
            .arg(fixture("representative/representative.mkv"))
            .args(["-map", "0:v:0", "-vf", "scale=w=1920:h=800"])
            .args(["-c:v", "libx264", "-preset", "veryfast", "-bf", "3"])
            .args(["-force_key_frames", "expr:gte(t,n_forced*2)"])
            .args(["-x264-params", "keyint=1000:min-keyint=1000:scenecut=0"])
            .arg(temp.path().join(name))
            .status()
            .await
            .unwrap();
        assert!(status.success());
        let out = temp.path().join("reduced.mp4");
        let count = rebuild(temp.path(), name, "hls-v0-h720", &out).await;
        let frames = |path: PathBuf| async move {
            let output = tokio::process::Command::new("ffprobe")
                .args(["-v", "error", "-select_streams", "v:0", "-show_entries"])
                .args(["packet=pts_time,dts_time,flags", "-of", "csv=p=0"])
                .arg(path)
                .output()
                .await
                .unwrap();
            String::from_utf8(output.stdout)
                .unwrap()
                .lines()
                .map(|line| {
                    let mut fields = line.split(',');
                    let pts: f64 = fields.next().unwrap().parse().unwrap();
                    // Matroska stores no decode times (`N/A`).
                    let dts = fields.next().unwrap().parse().unwrap_or(f64::NAN);
                    (pts, dts, fields.next().unwrap().starts_with('K'))
                })
                .collect::<Vec<_>>()
        };
        let source = frames(temp.path().join(name)).await;
        let reduced = frames(out.clone()).await;
        let offset = segments::TIMELINE_OFFSET_MS as f64 / 1000.0;
        let mut want: Vec<f64> = source.iter().map(|f| f.0).collect();
        let mut got: Vec<f64> = reduced.iter().map(|f| f.0 - offset).collect();
        want.sort_by(f64::total_cmp);
        got.sort_by(f64::total_cmp);
        assert_eq!(got.len(), want.len(), "frame count");
        for (g, w) in got.iter().zip(&want) {
            assert!((g - w).abs() < 0.002, "frame at {w} s shown at {g} s");
        }
        for pair in reduced.windows(2) {
            assert!(pair[1].1 > pair[0].1, "decode times {pair:?}");
        }
        // Every segment boundary of the playlist falls on a keyframe of the rendition.
        let cut = playlist::segments(
            &read(temp.path().join(name).to_str().unwrap(), 0).unwrap(),
            playlist::TARGET_MS,
        );
        assert_eq!(cut.len(), count);
        for segment in &cut {
            let start = segment.start_ms as f64 / 1000.0 + offset;
            assert!(
                reduced.iter().any(|f| f.2 && (f.0 - start).abs() < 0.002),
                "no keyframe at {start}"
            );
        }
        let probe = tokio::process::Command::new("ffprobe")
            .args(["-v", "error", "-select_streams", "v:0", "-show_entries"])
            .args(["stream=codec_name,profile,width,height", "-of", "csv=p=0"])
            .arg(&out)
            .output()
            .await
            .unwrap();
        let stream = String::from_utf8(probe.stdout).unwrap();
        let (width, height) = renditions::fit(1920, 800, renditions::RENDITIONS[1]).unwrap();
        // FFmpeg 9 names the profile by number: 100 is High (`avc1.64…`).
        assert!(
            [
                format!("h264,High,{width},{height}"),
                format!("h264,100,{width},{height}")
            ]
            .contains(&stream.trim().to_owned()),
            "{stream}"
        );
        // Every frame decodes without an error (the Node build has no `-f null` encoder).
        let decode = tokio::process::Command::new("ffprobe")
            .args(["-v", "error", "-select_streams", "v:0", "-count_frames"])
            .args(["-show_entries", "stream=nb_read_frames", "-of", "csv=p=0"])
            .arg(&out)
            .output()
            .await
            .unwrap();
        assert!(
            decode.status.success() && decode.stderr.is_empty(),
            "{}",
            String::from_utf8_lossy(&decode.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&decode.stdout).trim(),
            want.len().to_string()
        );
        // Played as HLS by FFmpeg's demuxer: a master playlist written as the
        // Core does, the Node's media playlist, its init section and segments.
        let master = temp.path().join("master.m3u8");
        std::fs::write(
            &master,
            format!(
                "#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-INDEPENDENT-SEGMENTS\n\
                 #EXT-X-STREAM-INF:BANDWIDTH=4800000,CODECS=\"{}\",RESOLUTION={width}x{height}\n\
                 hls-v0-h720.m3u8\n",
                crate::transcode::h264_codec(720)
            ),
        )
        .unwrap();
        let played = tokio::process::Command::new("ffprobe")
            .args(["-v", "error", "-protocol_whitelist", "file"])
            .args(["-allowed_extensions", "ALL", "-extension_picky", "0"])
            .args(["-select_streams", "v:0", "-count_frames", "-show_entries"])
            .args(["stream=width,height,nb_read_frames", "-of", "csv=p=0"])
            .arg(&master)
            .output()
            .await
            .unwrap();
        assert_eq!(
            // The HLS program lists the stream a second time.
            String::from_utf8_lossy(&played.stdout)
                .lines()
                .next()
                .unwrap_or(""),
            format!("{width},{height},{}", want.len()),
            "{}",
            String::from_utf8_lossy(&played.stderr)
        );
    }

    /// A track native players cannot decode is converted to AAC segment by
    /// segment, served out of order: the joined rendition is AAC on one grid of
    /// 1024 samples over the whole film (no gap, no overlap), and it sounds like
    /// the source at every join as much as anywhere else (no click).
    #[tokio::test]
    #[ignore = "requires the Node image's FFmpeg"]
    async fn converted_audio_rendition_is_continuous_aac_over_the_whole_film() {
        use http::Method;
        use http_body_util::BodyExt;
        let root = fixture("representative");
        let name = std::path::Path::new("representative.mkv");
        let preparations = std::sync::Arc::new(crate::packaging::Preparations::default());
        let get = |component: String| {
            let preparations = preparations.clone();
            let root = root.clone();
            async move {
                let opened = crate::OpenedFile::open_in_root(&root, name).await.unwrap();
                let response = preparations
                    .serve_hls(
                        opened,
                        "fixture".into(),
                        request(&component).unwrap(),
                        Method::GET,
                    )
                    .await
                    .unwrap();
                assert_eq!(response.status(), http::StatusCode::OK, "{component}");
                response
                    .into_body()
                    .collect()
                    .await
                    .unwrap()
                    .to_bytes()
                    .to_vec()
            }
        };
        let playlist = String::from_utf8(get("hls-v0-aac1.m3u8".into()).await).unwrap();
        let count = playlist.lines().filter(|l| l.ends_with(".m4s")).count();
        let mut joined = get("hls-v0-aac1-init.mp4".into()).await;
        let mut parts = Vec::new();
        for i in (0..count).rev() {
            parts.push((i, get(format!("hls-v0-aac1-{i}.m4s")).await));
        }
        parts.sort_by_key(|(i, _)| *i);
        parts.into_iter().for_each(|(_, part)| joined.extend(part));
        let temp = tempfile::tempdir().unwrap();
        let out = temp.path().join("audio.mp4");
        std::fs::write(&out, &joined).unwrap();
        let probe = tokio::process::Command::new("ffprobe")
            .args(["-v", "error", "-select_streams", "a:0", "-show_entries"])
            .args(["stream=codec_name:packet=pts_time", "-of", "csv=p=0"])
            .arg(&out)
            .output()
            .await
            .unwrap();
        let text = String::from_utf8(probe.stdout).unwrap();
        // ffprobe lists the packets, then the stream.
        assert!(
            text.lines().any(|l| l.trim_end_matches(',') == "aac"),
            "{text}"
        );
        let times: Vec<f64> = text
            .lines()
            .filter_map(|l| l.trim_end_matches(',').parse().ok())
            .collect();
        // One AAC frame is 1024 samples (21.3 ms at 48 kHz): no gap, no overlap.
        for pair in times.windows(2) {
            let step = pair[1] - pair[0];
            assert!(
                (0.015..0.03).contains(&step),
                "join at {}: step {step}",
                pair[0]
            );
        }
        let covered = times.last().unwrap() - times.first().unwrap();
        let film = read("representative/representative.mkv", 0)
            .unwrap()
            .duration_ms as f64
            / 1000.0;
        assert!(
            (covered - film).abs() < 0.2,
            "{covered} s of audio for a {film} s film"
        );
        // Exactly one frame apart, on the film-wide grid.
        let frame = 1024.0 / 48_000.0;
        for (i, time) in times.iter().enumerate() {
            let expected = 10.0 + i as f64 * frame;
            assert!((time - expected).abs() < 1e-4, "packet {i} at {time}");
        }
        // Mono PCM of the source track and of the joined rendition, same origin.
        async fn pcm(path: PathBuf, stream: &str, start: f64) -> Vec<f32> {
            let output = tokio::process::Command::new("ffmpeg")
                .args(["-v", "error", "-i"])
                .arg(path)
                .args(["-map", stream, "-af"])
                .arg(format!("atrim=start={start}"))
                .args(["-ac", "1", "-ar", "48000", "-c:a", "pcm_s16le"])
                .args(["-f", "s16le", "pipe:1"])
                .output()
                .await
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            output
                .stdout
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| f32::from(i16::from_le_bytes(*b)) / 32768.0)
                .collect::<Vec<f32>>()
        }
        let source = pcm(root.join(name), "0:1", 0.0).await;
        let converted = pcm(out.clone(), "0:a:0", 10.0).await;
        let length = source.len().min(converted.len());
        let error = |from: usize, to: usize| {
            let (signal, noise) = (from..to.min(length)).fold((0.0f64, 0.0f64), |(s, n), i| {
                let difference = f64::from(converted[i] - source[i]);
                (s + f64::from(source[i]).powi(2), n + difference.powi(2))
            });
            (signal, noise)
        };
        let (signal, noise) = error(0, length);
        let snr = 10.0 * (signal / noise.max(1e-12)).log10();
        assert!(snr > 15.0, "{snr} dB over the film: misaligned?");
        let typical = noise / length as f64;
        let frames = read("representative/representative.mkv", 0).unwrap();
        for segment in playlist::segments(&frames, playlist::TARGET_MS)
            .iter()
            .skip(1)
        {
            let join = (segment.start_ms * 48).div_ceil(1024) as usize * 1024;
            let (_, around) = error(join - 512, join + 512);
            assert!(
                around / 1024.0 < typical * 4.0 + 1e-9,
                "join at {} ms: {:.2e} vs {:.2e} typical",
                segment.start_ms,
                around / 1024.0,
                typical
            );
        }
    }

    #[test]
    fn audio_streams_unknown_streams_and_other_containers_are_refused() {
        if !fixture("av1-opus.mkv").exists() {
            return;
        }
        for (name, stream) in [
            ("av1-opus.mkv", 1),
            ("av1-opus.mp4", 1),
            ("av1-opus.mkv", 9),
        ] {
            assert_eq!(
                read(name, stream).unwrap_err().kind(),
                ErrorKind::Unsupported,
                "{name}:{stream}"
            );
        }
        if fixture("av1-opus-fragmented.mp4").exists() {
            assert_eq!(
                read("av1-opus-fragmented.mp4", 0).unwrap_err().kind(),
                ErrorKind::Unsupported
            );
        }
        // Another container (Ogg), written here: CI does not generate every fixture.
        let temp = tempfile::tempdir().unwrap();
        let ogg = temp.path().join("audio.opus");
        std::fs::write(&ogg, b"OggS\0\x02\0\0\0\0\0\0\0\0 not really a stream").unwrap();
        let error = keyframes::read(&mut File::open(&ogg).unwrap(), 0).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Unsupported);
    }
}
