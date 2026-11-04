use super::*;
use crate::files::OpenedFile;
use bytes::Bytes;
use http::{Method, Request, StatusCode, header};
use http_body_util::BodyExt;
use std::{
    fs::File,
    io::{self, Seek},
    sync::Arc,
    time::Instant,
};

use std::{path::Path, process::Stdio};
use tokio::process::Command;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cold_index_admission_rejects_sixty_fifth_and_releases_cancelled_waiters() {
    let native = Arc::new(NativeAudio::default());
    let held = native.indexing.clone().lock_owned().await;
    let mut requests = Vec::new();
    for _ in 0..64 {
        let native = native.clone();
        let source = opened().await;
        requests.push(tokio::spawn(async move {
            native.serve(source, 2, 0, &Request::new(())).await
        }));
    }
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while native.cold_admission.available_permits() != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let error = native
        .serve(opened().await, 2, 0, &Request::new(()))
        .await
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
    let cancelled = requests.remove(0);
    cancelled.abort();
    assert!(cancelled.await.unwrap_err().is_cancelled());
    assert_eq!(native.cold_admission.available_permits(), 1);
    let replacement = {
        let native = native.clone();
        let source = opened().await;
        tokio::spawn(async move { native.serve(source, 2, 0, &Request::new(())).await })
    };
    drop(held);
    for request in requests {
        assert_eq!(request.await.unwrap().unwrap().status(), StatusCode::OK);
    }
    assert_eq!(replacement.await.unwrap().unwrap().status(), StatusCode::OK);
    assert_eq!(native.cold_admission.available_permits(), 64);
    // Established index hits bypass cold admission and its FIFO entirely.
    let _cold = native.cold_admission.acquire_many(64).await.unwrap();
    let _gate = native.indexing.clone().lock_owned().await;
    assert_eq!(
        native
            .serve(opened().await, 2, 0, &Request::new(()))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelling_index_request_retains_admission_until_blocking_work_finishes() {
    let native = Arc::new(NativeAudio::default());
    let indexing = native.indexing.clone();
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let request = tokio::spawn(async move {
        let guard = indexing.lock_owned().await;
        guarded_index_task(guard, move || {
            let _ = started_tx.send(());
            release_rx.recv().map_err(io::Error::other)?;
            Ok(())
        })
        .await
    });
    started_rx.await.unwrap();
    request.abort();
    assert!(request.await.unwrap_err().is_cancelled());
    let blocked = native.indexing.try_lock().is_err();
    // Release the worker before asserting, so a failed regression never
    // strands an unabortable blocking task during runtime shutdown.
    release_tx.send(()).unwrap();
    let guard = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        native.indexing.clone().lock_owned(),
    )
    .await
    .unwrap();
    drop(guard);
    assert!(
        blocked,
        "HTTP cancellation must not admit overlapping blocking index builds"
    );
    let response = native
        .serve(opened().await, 2, 0, &Request::new(()))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

fn locate_samples_reference(
    track: &Track,
    data: &[u8],
    selected: &mut [Sample],
) -> io::Result<u64> {
    // Previous production algorithm, retained only for equivalence and the
    // ignored CPU microbenchmark. No file I/O or decoding is timed here.
    let runs = track.chunk_runs(data)?;
    let mut total = 0u64;
    for sample in selected {
        let run = runs.partition_point(|&(_, _, _, base)| base <= sample.ordinal);
        let &(first, _, per, base) = runs
            .get(run.saturating_sub(1))
            .ok_or_else(|| invalid("sample missing chunk"))?;
        let chunk = first + (sample.ordinal - base) / per;
        let chunk_first = base + (chunk - first) * per;
        sample.offset = track.offset(data, chunk)? + track.byte_sum(data, sample.ordinal)?
            - track.byte_sum(data, chunk_first)?;
        total += u64::from(sample.size);
    }
    Ok(total)
}

#[test]
fn sample_offsets_match_reference_across_variable_sizes_and_chunk_runs() {
    let mut data = Vec::new();
    for size in 1u32..=30 {
        data.extend(size.to_be_bytes());
    }
    let stsc = data.len();
    for entry in [[1u32, 3, 1], [5, 6, 1]] {
        for value in entry {
            data.extend(value.to_be_bytes());
        }
    }
    let offsets = data.len();
    for value in [1000u32, 1010, 1030, 1060, 1100, 1200, 1400] {
        data.extend(value.to_be_bytes());
    }
    let track = Track {
        id: 2,
        count: 30,
        media_time: 0,
        duration: 30 * 960,
        stts: (0, 0),
        stsc: (stsc, 2),
        sizes: 0,
        fixed_size: 0,
        offsets: (offsets, 7),
        wide_offsets: false,
        byte_prefix: vec![0],
        init: Bytes::new(),
    };
    let mut samples = (2..27)
        .map(|ordinal| Sample {
            ordinal,
            offset: 0,
            size: ordinal + 1,
            duration: 960,
            dts: u64::from(ordinal) * 960,
            roll: 4,
        })
        .collect::<Vec<_>>();
    let mut reference = samples.clone();
    let expected = locate_samples_reference(&track, &data, &mut reference).unwrap();
    assert_eq!(track.locate_samples(&data, &mut samples).unwrap(), expected);
    assert_eq!(samples, reference);
    assert_eq!(samples[0].offset, 1003);
    assert_eq!(samples[10].offset, 1100);
    assert_eq!(samples[24].offset, 1451);
}

#[test]
fn a_seek_retains_preroll_history_across_short_timing_runs() {
    let runs = [(447u32, 960u32), (1, 1920), (2, 480), (20, 5760)];
    let count = runs.iter().map(|r| r.0).sum::<u32>();
    let mut data = Vec::new();
    for (count, delta) in runs {
        data.extend(count.to_be_bytes());
        data.extend(delta.to_be_bytes());
    }
    let stsc = data.len();
    for value in [1, count, 1] {
        data.extend(value.to_be_bytes());
    }
    let offsets = data.len();
    data.extend(1000u32.to_be_bytes());
    let track = Track {
        id: 2,
        count,
        media_time: 0,
        duration: runs
            .iter()
            .map(|&(n, delta)| u64::from(n) * u64::from(delta))
            .sum(),
        stts: (0, 4),
        stsc: (stsc, 1),
        sizes: 0,
        fixed_size: 1,
        offsets: (offsets, 1),
        wide_offsets: false,
        byte_prefix: vec![0, 256],
        init: Bytes::new(),
    };
    let window = track.window(&data, 1).unwrap();
    let first = &window.samples[0];
    assert_eq!(first.ordinal, 450);
    assert_eq!(first.dts, 9 * 48000);
    // Preceding packet durations are 10+10+40+20 ms. Three packets would
    // contain only 60 ms; the 120 ms *current* packet cannot fill that gap.
    assert_eq!(first.roll, 4);
}

#[test]
#[ignore = "CPU microbenchmark; warm moov metadata only, no browser or disk latency"]
fn benchmark_native_sample_offsets() {
    let fixture = std::env::var_os("MEDIA_NODE_WINDOW_BENCHMARK_FIXTURE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root().join("representative.mp4"));
    let index = load_index(File::open(&fixture).unwrap()).unwrap();
    let track = &index.tracks[0];
    let mut reports = Vec::new();
    for chunk in [0, 1] {
        let original = track.window(&index.data, chunk).unwrap().samples;
        let mut baseline = original.clone();
        let mut candidate = original.clone();
        locate_samples_reference(track, &index.data, &mut baseline).unwrap();
        track.locate_samples(&index.data, &mut candidate).unwrap();
        assert_eq!(baseline, candidate);
        for reference in [true, false, false, true, true, false] {
            let start = Instant::now();
            let mut samples = original.clone();
            for _ in 0..512 {
                let result = if reference {
                    locate_samples_reference(track, std::hint::black_box(&index.data), &mut samples)
                } else {
                    track.locate_samples(std::hint::black_box(&index.data), &mut samples)
                }
                .unwrap();
                std::hint::black_box(result);
                std::hint::black_box(&samples);
            }
            reports.push(serde_json::json!({"strategy":if reference{"prefix_per_packet"}else{"chunk_cursor"},"chunk":chunk,"packets_per_window":samples.len(),"iterations":512,"elapsed_ms":start.elapsed().as_secs_f64()*1000.0}));
        }
    }
    println!(
        "{}",
        serde_json::json!({"scope":"Rust offset planning only; moov/index warm; no source payload IO, network, browser or NAS claim","moov_bytes":index.data.len(),"results":reports})
    );
}

#[test]
fn variable_packet_durations_resolve_fragment_local_roll_descriptions() {
    // One legal Opus timing sequence with 10, 20 and 40 ms packets. A seek
    // needs three prior packets at sample 3, then four at 4..6, five at 7.
    // Reusing init's generic -4 would be wrong even though hashes/PTS pass.
    let deltas = [960u32, 960, 1920, 480, 480, 960, 960, 960];
    let mut data = Vec::new();
    for delta in deltas {
        data.extend(1u32.to_be_bytes());
        data.extend(delta.to_be_bytes());
    }
    let stsc = data.len();
    data.extend(1u32.to_be_bytes());
    data.extend(8u32.to_be_bytes());
    data.extend(1u32.to_be_bytes());
    let offsets = data.len();
    data.extend(1024u32.to_be_bytes());
    let global = roll_boxes(&[4], &[0], false);
    let track = Track {
        id: 2,
        count: 8,
        media_time: 0,
        duration: deltas.iter().map(|d| u64::from(*d)).sum(),
        stts: (0, 8),
        stsc: (stsc, 1),
        sizes: 0,
        fixed_size: 1,
        offsets: (offsets, 1),
        wide_offsets: false,
        byte_prefix: vec![0],
        init: Bytes::copy_from_slice(&global),
    };
    let window = track.window(&data, 0).unwrap();
    let expected = window
        .samples
        .iter()
        .map(|sample| -(sample.roll as i16))
        .collect::<Vec<_>>();
    assert_eq!(&expected[3..], &[-3, -4, -4, -4, -5]);
    // The virtual prefix ends at the mdat header; payload is streamed later.
    let boxes = atoms(&window.prefix, 0..window.prefix.len() - 8).unwrap();
    let moof = one(&boxes, b"moof").unwrap();
    let traf = one(&children(&window.prefix, moof).unwrap(), b"traf").unwrap();
    let fragment = children(&window.prefix, traf).unwrap();
    let global_descriptions = one(&boxes, b"sgpd").unwrap().data(&window.prefix);
    let descriptions = one(&fragment, b"sgpd").unwrap().data(&window.prefix);
    let mapping = one(&fragment, b"sbgp").unwrap().data(&window.prefix);
    let count = u32be(mapping, 8).unwrap();
    let mut resolved = Vec::new();
    for entry in 0..count {
        let base = 12 + entry as usize * 8;
        let samples = u32be(mapping, base).unwrap();
        let index = u32be(mapping, base + 4).unwrap();
        assert!(
            index >= 0x10001,
            "traf must reference its own variable roll descriptions"
        );
        let (table, index) = if index >= 0x10001 {
            (descriptions, index - 0x10000)
        } else {
            (global_descriptions, index)
        };
        assert!(index > 0 && index <= u32be(table, 12).unwrap());
        let distance = u16be(table, 16 + (index - 1) as usize * 2).unwrap() as i16;
        resolved.extend(std::iter::repeat_n(distance, samples as usize));
    }
    assert_eq!(resolved, expected);
    let global_mapping = one(&boxes, b"sbgp").unwrap().data(&window.prefix);
    assert_eq!(
        u32be(global_mapping, 8).unwrap(),
        0,
        "initialization maps zero samples"
    );
    assert_eq!(u32be(global_descriptions, 12).unwrap(), 1);
    assert_eq!(
        u16be(global_descriptions, 16).unwrap() as i16,
        -4,
        "track-level roll description stays available"
    );
}

fn root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/generated/representative")
}
async fn opened() -> OpenedFile {
    OpenedFile::open_in_root(&root(), Path::new("representative.mp4"))
        .await
        .unwrap()
}
async fn packets(file: &OpenedFile, track: &str) -> serde_json::Value {
    let mut input = file.try_clone_std().unwrap();
    input.rewind().unwrap();
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-protocol_whitelist",
            "fd,pipe",
            "-select_streams",
            track,
            "-show_streams",
            "-show_packets",
            "-show_data_hash",
            "sha256",
            "-show_entries",
            "stream=channels,codec_name:packet=pts_time,dts_time,data_hash",
            "-of",
            "json",
            "-",
        ])
        .stdin(Stdio::from(input))
        .output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
#[tokio::test]
async fn native_windows_preserve_opus_packets_timestamps_channels_and_track_ids() {
    let native = NativeAudio::default();
    assert_eq!(
        supported_tracks(opened().await.try_clone_std().unwrap()).unwrap(),
        vec![2, 3]
    );
    let temp = tempfile::tempdir().unwrap();
    for (track, selection, channels) in [(2, "a:0", 6), (3, "a:1", 2)] {
        let source = packets(&opened().await, selection).await;
        for chunk in [0, 1] {
            let response = native
                .serve(opened().await, track, chunk, &Request::new(()))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let length = response.headers()[header::CONTENT_LENGTH]
                .to_str()
                .unwrap()
                .parse::<usize>()
                .unwrap();
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            assert_eq!(length, bytes.len());
            std::fs::write(temp.path().join("audio.mp4"), &bytes).unwrap();
            let remux = OpenedFile::open_in_root(temp.path(), Path::new("audio.mp4"))
                .await
                .unwrap();
            let copied = packets(&remux, "a:0").await;
            assert_eq!(copied["streams"][0]["codec_name"], "opus");
            assert_eq!(copied["streams"][0]["channels"], channels);
            let actual = copied["packets"].as_array().unwrap();
            assert!(!actual.is_empty());
            if chunk == 1 {
                assert!(
                    actual[0]["pts_time"]
                        .as_str()
                        .unwrap()
                        .parse::<f64>()
                        .unwrap()
                        > 8.9,
                    "timestamps must stay absolute"
                );
            }
            for packet in actual {
                assert!(
                    source["packets"].as_array().unwrap().iter().any(
                        |original| original["data_hash"] == packet["data_hash"]
                            && (original["pts_time"]
                                .as_str()
                                .unwrap()
                                .parse::<f64>()
                                .unwrap()
                                - packet["pts_time"].as_str().unwrap().parse::<f64>().unwrap())
                            .abs()
                                < 0.000022
                    ),
                    "packet payload and presentation timestamp must match source: {packet}"
                );
            }
        }
    }
    assert_eq!(native.cache.lock().await.len(), 1);
}
#[tokio::test]
async fn virtual_ranges_match_full_representation_head_and_cancellation() {
    let native = NativeAudio::default();
    let response = native
        .serve(opened().await, 2, 0, &Request::new(()))
        .await
        .unwrap();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    for (start, end) in [(0, 11), (512, 4096), (bytes.len() - 99, bytes.len() - 1)] {
        let request = Request::builder()
            .header(header::RANGE, format!("bytes={start}-{end}"))
            .body(())
            .unwrap();
        let response = native.serve(opened().await, 2, 0, &request).await.unwrap();
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(
            response.into_body().collect().await.unwrap().to_bytes(),
            bytes.slice(start..end + 1)
        );
    }
    let head = Request::builder().method(Method::HEAD).body(()).unwrap();
    let response = native.serve(opened().await, 2, 0, &head).await.unwrap();
    assert_eq!(
        response.headers()[header::CONTENT_LENGTH],
        bytes.len().to_string()
    );
    assert!(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .is_empty()
    );
    for range in ["bytes=999999999-", "bytes=5-1", "bytes=1-2,3-4"] {
        let request = Request::builder()
            .header(header::RANGE, range)
            .body(())
            .unwrap();
        let response = native.serve(opened().await, 2, 0, &request).await.unwrap();
        assert_eq!(response.status(), StatusCode::RANGE_NOT_SATISFIABLE);
    }
    let mut response = native
        .serve(opened().await, 2, 0, &Request::new(()))
        .await
        .unwrap();
    response.body_mut().frame().await.unwrap().unwrap();
    drop(response);
    let response = native
        .serve(opened().await, 3, 1, &Request::new(()))
        .await
        .unwrap();
    assert!(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .len()
            < MAX_WINDOW_BYTES as usize
    );
}
#[tokio::test]
async fn unsupported_tracks_out_of_bounds_windows_and_changed_sources_fail() {
    let native = NativeAudio::default();
    assert_eq!(
        native
            .serve(opened().await, 1, 0, &Request::new(()))
            .await
            .unwrap_err()
            .kind(),
        io::ErrorKind::Unsupported
    );
    assert_eq!(
        native
            .serve(opened().await, 2, 8640, &Request::new(()))
            .await
            .unwrap_err()
            .kind(),
        io::ErrorKind::Unsupported
    );
    assert_eq!(
        native
            .serve(opened().await, 2, 2, &Request::new(()))
            .await
            .unwrap_err()
            .kind(),
        io::ErrorKind::Unsupported
    );
    let temp = tempfile::tempdir().unwrap();
    std::fs::copy(
        root().join("representative.mp4"),
        temp.path().join("changed.mp4"),
    )
    .unwrap();
    let file = OpenedFile::open_in_root(temp.path(), Path::new("changed.mp4"))
        .await
        .unwrap();
    std::fs::write(temp.path().join("changed.mp4"), b"changed").unwrap();
    assert!(native.serve(file, 2, 0, &Request::new(())).await.is_err());
}
#[test]
fn rejects_malformed_boxes_huge_moov_external_references_and_invalid_tables() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("bad.mp4");
    for bytes in [
        vec![0; 7],
        vec![0, 0, 0, 4, b'm', b'o', b'o', b'v'],
        vec![1, 0, 0, 0, b'm', b'o', b'o', b'v'],
    ] {
        std::fs::write(&path, bytes).unwrap();
        assert!(supported_tracks(File::open(&path).unwrap()).is_err());
    }
    let source = std::fs::read(root().join("representative.mp4")).unwrap();
    for kind in [b"dOps", b"stts", b"stsc", b"stsz", b"stco", b"dref"] {
        let mut bad = source.clone();
        // Invalid version or flags in each required table/config/reference.
        for at in source
            .windows(4)
            .enumerate()
            .filter_map(|(at, bytes)| (bytes == kind).then_some(at))
        {
            bad[at + 4] = 255;
        }
        std::fs::write(&path, bad).unwrap();
        let ids = supported_tracks(File::open(&path).unwrap());
        assert!(
            ids.is_err() || !ids.unwrap().contains(&2),
            "mutated {kind:?} must never advertise track 2"
        );
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_native_windows_share_one_index_without_a_preparation_worker() {
    for sessions in [4, 8, 16] {
        let native = Arc::new(NativeAudio::default());
        let mut tasks = tokio::task::JoinSet::new();
        for n in 0..sessions {
            let native = native.clone();
            tasks.spawn(async move {
                let response = native
                    .serve(
                        opened().await,
                        if n % 2 == 0 { 2 } else { 3 },
                        n % 2,
                        &Request::new(()),
                    )
                    .await
                    .unwrap();
                let mut body = response.into_body();
                let mut total = 0;
                while let Some(frame) = body.frame().await {
                    let data = frame.unwrap().into_data().unwrap();
                    assert!(data.len() <= BLOCK);
                    total += data.len();
                }
                assert!(total > 0);
            });
        }
        while let Some(result) = tasks.join_next().await {
            result.unwrap();
        }
        assert_eq!(native.cache.lock().await.len(), 1);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn malformed_concurrent_requests_do_not_poison_the_native_index_service() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("invalid.mp4"), b"not an MP4").unwrap();
    let native = Arc::new(NativeAudio::default());
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..16 {
        let native = native.clone();
        let root = temp.path().to_owned();
        tasks.spawn(async move {
            let source = OpenedFile::open_in_root(&root, Path::new("invalid.mp4"))
                .await
                .unwrap();
            assert!(native.serve(source, 2, 0, &Request::new(())).await.is_err());
        });
    }
    while let Some(result) = tasks.join_next().await {
        result.unwrap();
    }
    assert!(native.cache.lock().await.is_empty());
    let response = native
        .serve(opened().await, 2, 0, &Request::new(()))
        .await
        .unwrap();
    assert!(
        !response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .is_empty()
    );
}
