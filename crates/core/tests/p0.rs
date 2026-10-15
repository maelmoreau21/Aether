use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::{BodyExt, Empty};
use media_poc::{app, media, node::Inventory};
use std::path::PathBuf;
use tower::ServiceExt;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/generated")
}
async fn router() -> axum::Router {
    app(
        Inventory::load(root())
            .await
            .expect("Run scripts/generate-fixtures.ps1 first"),
        3000,
    )
}
fn request(method: &str, path: &str) -> axum::http::request::Builder {
    Request::builder()
        .method(method)
        .uri(path)
        .header("host", "127.0.0.1:3000")
}

#[tokio::test]
async fn mse_fixed_inventory_ranges_and_errors() {
    let app = router().await;
    for (name, mime) in [
        ("video.mp4", "video/mp4"),
        ("audio-fr.mp4", "audio/mp4"),
        ("audio-en.mp4", "audio/mp4"),
        ("fr-full.vtt", "text/vtt; charset=utf-8"),
        ("fr-forced.vtt", "text/vtt; charset=utf-8"),
        ("en-full.vtt", "text/vtt; charset=utf-8"),
    ] {
        let uri = format!("/api/v1/experiment/mse/{name}");
        let response = app
            .clone()
            .oneshot(
                request("GET", &uri)
                    .header("range", "bytes=0-7")
                    .body(Empty::<axum::body::Bytes>::new())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(response.headers()["content-type"], mime);
        let actual = response.into_body().collect().await.unwrap().to_bytes();
        let expected = std::fs::read(root().join("mse/verified").join(name)).unwrap();
        assert_eq!(actual.as_ref(), &expected[..8]);
        let response = app
            .clone()
            .oneshot(
                request("HEAD", &uri)
                    .header("range", "bytes=99999999-")
                    .body(Empty::<axum::body::Bytes>::new())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::RANGE_NOT_SATISFIABLE);
    }
    for name in ["generation.json", "missing.mp4", "representative.mkv"] {
        let response = app
            .clone()
            .oneshot(
                request("GET", &format!("/api/v1/experiment/mse/{name}"))
                    .body(Empty::<axum::body::Bytes>::new())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
    let response = app
        .oneshot(
            request("GET", "/mse")
                .header("origin", "https://example.com")
                .body(Empty::<axum::body::Bytes>::new())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn dash_fixed_inventory_and_manifest_ranges() {
    let app = router().await;
    let script = app
        .clone()
        .oneshot(
            request("GET", "/audio-seek-policy.js")
                .body(Empty::<axum::body::Bytes>::new())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(script.status(), StatusCode::OK);
    assert_eq!(
        script.headers()["content-type"],
        "text/javascript; charset=utf-8"
    );
    let rejected = app
        .clone()
        .oneshot(
            request("GET", "/audio-seek-policy.js")
                .header("origin", "https://example.com")
                .body(Empty::<axum::body::Bytes>::new())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(rejected.status(), StatusCode::FORBIDDEN);
    for (name, mime) in [
        ("manifest.mpd", "application/dash+xml"),
        ("video.mp4", "video/mp4"),
        ("audio-fr.mp4", "audio/mp4"),
        ("audio-en.mp4", "audio/mp4"),
        ("fr-full.vtt", "text/vtt; charset=utf-8"),
        ("fr-forced.vtt", "text/vtt; charset=utf-8"),
        ("en-full.vtt", "text/vtt; charset=utf-8"),
    ] {
        let response = app
            .clone()
            .oneshot(
                request("GET", &format!("/api/v1/experiment/shaka/{name}"))
                    .header("range", "bytes=0-15")
                    .body(Empty::<axum::body::Bytes>::new())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(response.headers()["content-type"], mime);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let expected = std::fs::read(root().join("dash").join(name)).unwrap();
        assert_eq!(bytes.as_ref(), &expected[..16]);
    }
    for name in ["generation.json", "LICENSE", "player.js", "missing.mp4"] {
        let response = app
            .clone()
            .oneshot(
                request("GET", &format!("/api/v1/experiment/shaka/{name}"))
                    .body(Empty::<axum::body::Bytes>::new())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
    let response = app
        .oneshot(
            request("GET", "/shaka")
                .header("sec-fetch-site", "cross-site")
                .body(Empty::<axum::body::Bytes>::new())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn real_containers_all_tracks_and_invalid_media() {
    let inventory = Inventory::load(root()).await.unwrap();
    for asset in inventory
        .descriptors()
        .into_iter()
        .filter(|a| a.language_remux.is_none() && a.representative.is_none())
    {
        assert_eq!(asset.probe.streams.len(), 3);
        assert_eq!(asset.probe.streams[0].codec_name.as_deref(), Some("av1"));
        assert_eq!(asset.probe.streams[1].codec_name.as_deref(), Some("opus"));
        assert_eq!(asset.probe.streams[2].codec_name.as_deref(), Some("opus"));
        assert!(asset.probe.streams.iter().all(|s| s.time_base.is_some()));
        assert!(!asset.probe.format.contains_key("filename"));
        assert_eq!(asset.probe.streams[1].tags["language"], "fra");
        assert_eq!(asset.probe.streams[2].tags["language"], "eng");
    }
    let invalid = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(invalid.path(), b"not a media file").unwrap();
    assert!(matches!(
        media::probe(invalid.path()).await,
        Err(media::ProbeError::Rejected)
    ));
    let truncated = tempfile::NamedTempFile::new().unwrap();
    let bytes = std::fs::read(root().join("av1-opus.mp4")).unwrap();
    std::fs::write(truncated.path(), &bytes[..24]).unwrap();
    assert!(media::probe(truncated.path()).await.is_err());
}

#[tokio::test]
async fn representative_tracks_and_ten_bit_profile() {
    let inventory = Inventory::load(root()).await.unwrap();
    let cases: Vec<_> = inventory
        .descriptors()
        .into_iter()
        .filter(|a| a.representative.is_some())
        .collect();
    assert_eq!(
        cases.len(),
        4,
        "Run python scripts/generate-representative-fixtures.py first"
    );
    for asset in cases {
        let case = asset.representative.as_ref().unwrap();
        let video = &asset.probe.streams[0];
        assert_eq!(video.codec_name.as_deref(), Some("av1"));
        assert_eq!(video.details["profile"], "Main");
        assert_eq!(video.details["pix_fmt"], "yuv420p10le");
        assert_eq!(video.details["color_space"], "bt709");
        let audio: Vec<_> = asset
            .probe
            .streams
            .iter()
            .filter(|s| s.codec_type.as_deref() == Some("audio"))
            .collect();
        assert_eq!(
            audio.len(),
            if case.expected_audio_language == "fr" {
                2
            } else {
                1
            }
        );
        assert_eq!(audio[0].details["channels"], case.expected_audio_channels);
        assert!(
            audio
                .iter()
                .all(|s| s.codec_name.as_deref() == Some("opus"))
        );
        if audio.len() == 2 {
            assert_eq!(audio[1].details["channels"], 2);
            assert_eq!(audio[1].tags["language"], "eng");
        }
        let subs: Vec<_> = asset
            .probe
            .streams
            .iter()
            .filter(|s| s.codec_type.as_deref() == Some("subtitle"))
            .collect();
        assert_eq!(subs.len(), 3);
        assert_eq!(subs[0].tags["language"], "fra");
        assert_eq!(subs[1].tags["language"], "fra");
        assert_eq!(subs[2].tags["language"], "eng");
        assert_eq!(subs[1].details["disposition"]["forced"], 1);
        let codec = if asset.mime == "video/mp4" {
            "mov_text"
        } else {
            "subrip"
        };
        assert!(subs.iter().all(|s| s.codec_name.as_deref() == Some(codec)));
    }
    let partial = tempfile::tempdir().unwrap();
    for (_, filename, _) in media_poc::node::FIXTURES {
        std::fs::copy(root().join(filename), partial.path().join(filename)).unwrap();
    }
    std::fs::create_dir(partial.path().join("representative")).unwrap();
    assert!(Inventory::load(partial.path().to_path_buf()).await.is_err());
    let truncated = tempfile::NamedTempFile::new().unwrap();
    let bytes = std::fs::read(root().join("representative/representative.mp4")).unwrap();
    std::fs::write(truncated.path(), &bytes[..16]).unwrap();
    assert!(media::probe(truncated.path()).await.is_err());
}

#[tokio::test]
async fn http_head_ranges_and_exact_bytes() {
    let router = router().await;
    for (id, name, _) in media_poc::node::FIXTURES
        .into_iter()
        .chain(media_poc::node::REMUX_FIXTURES)
        .chain(media_poc::node::REPRESENTATIVE_FIXTURES)
    {
        let path = format!("/api/v1/assets/{id}/content");
        let expected = std::fs::read(root().join(name)).unwrap();
        for method in ["HEAD", "GET"] {
            let res = router
                .clone()
                .oneshot(request(method, &path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::OK);
            assert_eq!(res.headers()["content-length"], expected.len().to_string());
            assert_eq!(res.headers()["accept-ranges"], "bytes");
            let bytes = res.into_body().collect().await.unwrap().to_bytes();
            if method == "HEAD" {
                assert!(bytes.is_empty());
            } else {
                assert_eq!(&bytes[..], expected);
            }
        }
        for (range, start, end) in [
            ("bytes=0-31".to_string(), 0, 31),
            ("bytes=-16".into(), expected.len() - 16, expected.len() - 1),
            (
                format!("bytes={}-", expected.len() - 20),
                expected.len() - 20,
                expected.len() - 1,
            ),
            (
                format!("bytes=10-{}", expected.len() + 100),
                10,
                expected.len() - 1,
            ),
        ] {
            let res = router
                .clone()
                .oneshot(
                    request("GET", &path)
                        .header("range", range)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::PARTIAL_CONTENT);
            assert_eq!(
                res.headers()["content-range"],
                format!("bytes {start}-{end}/{}", expected.len())
            );
            assert_eq!(
                res.into_body().collect().await.unwrap().to_bytes().as_ref(),
                &expected[start..=end]
            );
        }
        for range in [
            "bytes=999999999-",
            "bytes=10-2",
            "bytes=-0",
            "bytes=nope",
            "bytes=0-1,4-5",
        ] {
            let res = router
                .clone()
                .oneshot(
                    request("GET", &path)
                        .header("range", range)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::RANGE_NOT_SATISFIABLE, "{range}");
            assert_eq!(
                res.headers()["content-range"],
                format!("bytes */{}", expected.len())
            );
        }
    }
}

#[tokio::test]
async fn security_and_unknown_ids() {
    let router = router().await;
    for path in [
        "/api/v1/assets/nope/content",
        "/api/v1/assets/fixture-mkv-xx/content",
        "/api/v1/assets/%2e%2e%2fMEMORY.md/content",
        "/api/v1/assets/C%3A%5CWindows%5Cwin.ini/content",
    ] {
        let res = router
            .clone()
            .oneshot(request("GET", path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }
    for (header, value) in [
        ("host", "attacker.invalid"),
        ("origin", "https://attacker.invalid"),
        ("sec-fetch-site", "cross-site"),
    ] {
        let mut req = request("GET", "/api/v1/assets")
            .body(Body::empty())
            .unwrap();
        req.headers_mut().insert(
            axum::http::HeaderName::from_bytes(header.as_bytes()).unwrap(),
            value.parse().unwrap(),
        );
        assert_eq!(
            router.clone().oneshot(req).await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(
        router
            .oneshot(
                request("POST", "/api/v1/assets/fixture-mkv/content")
                    .body(Body::empty())
                    .unwrap()
            )
            .await
            .unwrap()
            .status(),
        StatusCode::METHOD_NOT_ALLOWED
    );
}

#[tokio::test]
async fn language_remux_inventory_and_partial_preparation() {
    let descriptors = Inventory::load(root()).await.unwrap().descriptors();
    let remuxes: Vec<_> = descriptors
        .iter()
        .filter(|a| a.language_remux.is_some())
        .collect();
    assert_eq!(
        remuxes.len(),
        4,
        "Run scripts/prepare-language-remux.ps1 first"
    );
    for asset in remuxes {
        let remux = asset.language_remux.as_ref().unwrap();
        assert!(descriptors.iter().any(|a| a.id == remux.source_asset));
        assert_eq!(asset.probe.streams.len(), 2);
        assert_eq!(asset.probe.streams[0].codec_name.as_deref(), Some("av1"));
        assert_eq!(asset.probe.streams[1].codec_name.as_deref(), Some("opus"));
        let (source_index, tag) = if remux.language == "fr" {
            (1, "fra")
        } else {
            (2, "eng")
        };
        assert_eq!(remux.source_audio_stream, source_index);
        assert_eq!(asset.probe.streams[1].tags["language"], tag);
    }
    let temp = tempfile::tempdir().unwrap();
    for (_, filename, _) in media_poc::node::FIXTURES {
        std::fs::copy(root().join(filename), temp.path().join(filename)).unwrap();
    }
    // Optional preparation may be absent, but a partial directory must fail closed.
    assert_eq!(
        Inventory::load(temp.path().to_path_buf())
            .await
            .unwrap()
            .descriptors()
            .len(),
        4
    );
    std::fs::create_dir(temp.path().join("language-remux")).unwrap();
    assert!(Inventory::load(temp.path().to_path_buf()).await.is_err());
}

#[tokio::test]
async fn streaming_is_chunked_and_can_be_cancelled() {
    let router = router().await;
    let mut response = router
        .clone()
        .oneshot(
            request("GET", "/api/v1/assets/fixture-mkv/content")
                .body(Empty::<axum::body::Bytes>::new())
                .unwrap(),
        )
        .await
        .unwrap();
    let first = response
        .body_mut()
        .frame()
        .await
        .unwrap()
        .unwrap()
        .into_data()
        .unwrap();
    assert!(first.len() <= 65_536);
    assert!(
        first.len()
            < std::fs::metadata(root().join("av1-opus.mkv"))
                .unwrap()
                .len() as usize
    );
    drop(response);
    assert_eq!(
        router
            .oneshot(
                request("GET", "/api/v1/health")
                    .body(Body::empty())
                    .unwrap()
            )
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
}

#[cfg(unix)]
#[tokio::test]
async fn replaced_link_cannot_escape_root() {
    // Point the inventory at an outside symlink.
    // Linux CI can create links without Windows administrator privileges.
    #[cfg(unix)]
    {
        let dir = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(root().join("av1-opus.mkv"), dir.path().join("av1-opus.mkv"))
            .unwrap();
        assert!(Inventory::load(dir.path().to_path_buf()).await.is_err());
    }
}

#[tokio::test]
async fn large_file_is_not_buffered_in_full() {
    let temp = tempfile::tempdir().unwrap();
    for (_, filename, _) in media_poc::node::FIXTURES {
        std::fs::copy(root().join(filename), temp.path().join(filename)).unwrap();
    }
    let inventory = Inventory::load(temp.path().to_path_buf()).await.unwrap();
    // Enlarge a disposable COPY after probing; never touch original media.
    // This tests HTTP I/O, not validity of a 1 GiB movie or measured RSS.
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(temp.path().join("av1-opus.mkv"))
        .unwrap();
    file.set_len(1_073_741_824).unwrap();
    drop(file);
    let router = app(inventory, 3000);
    let mut res = router
        .oneshot(
            request("GET", "/api/v1/assets/fixture-mkv/content")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.headers()["content-length"], "1073741824");
    let frame = res
        .body_mut()
        .frame()
        .await
        .unwrap()
        .unwrap()
        .into_data()
        .unwrap();
    assert!(frame.len() <= 65_536);
    drop(res);
}
#[tokio::test]
async fn demo_contract_and_read_only_sources() {
    let app = router().await;
    let response = app
        .clone()
        .oneshot(
            request("GET", "/api/v1/demo/media")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let items = json["media"].as_array().unwrap();
    assert_eq!(items.len(), 2, "Run scripts/prepare-p1.py");
    assert!(!String::from_utf8_lossy(&bytes).contains("filename"));
    for item in items {
        assert_eq!(item["sources"].as_array().unwrap().len(), 2);
        for source in item["sources"].as_array().unwrap() {
            let tracks = source["tracks"].as_array().unwrap();
            assert_eq!(tracks.len(), 2);
            assert_eq!(tracks[0]["pix_fmt"], "yuv420p10le");
            assert_eq!(tracks[1]["codec_name"], "opus");
            assert_eq!(
                tracks[1]["tags"]["language"],
                if source["language"] == "fr" {
                    "fra"
                } else {
                    "eng"
                }
            );
            assert_eq!(
                tracks[1]["channels"],
                if source["language"] == "fr" { 6 } else { 2 }
            );
            let url = source["url"].as_str().unwrap();
            let response = app
                .clone()
                .oneshot(
                    request("GET", url)
                        .header("range", "bytes=0-15")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
            assert_eq!(
                response
                    .into_body()
                    .collect()
                    .await
                    .unwrap()
                    .to_bytes()
                    .len(),
                16
            );
            let response = app
                .clone()
                .oneshot(
                    request("HEAD", url)
                        .header("range", "bytes=999999999-")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::RANGE_NOT_SATISFIABLE);
        }
    }
    for name in ["fr-full.vtt", "fr-forced.vtt", "en-full.vtt"] {
        let response = app
            .clone()
            .oneshot(
                request("GET", &format!("/api/v1/demo/subtitles/{name}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()["content-type"],
            "text/vtt; charset=utf-8"
        );
    }
    for (method, uri, status) in [
        (
            "GET",
            "/api/v1/demo/subtitles/video.mp4",
            StatusCode::NOT_FOUND,
        ),
        ("POST", "/api/v1/demo/media", StatusCode::METHOD_NOT_ALLOWED),
    ] {
        assert_eq!(
            app.clone()
                .oneshot(request(method, uri).body(Body::empty()).unwrap())
                .await
                .unwrap()
                .status(),
            status
        );
    }
    assert_eq!(
        app.oneshot(
            request("GET", "/api/v1/demo/media")
                .header("origin", "https://example.com")
                .body(Body::empty())
                .unwrap()
        )
        .await
        .unwrap()
        .status(),
        StatusCode::FORBIDDEN
    );
}
