use super::*;
use media_contracts::inventory::CachedFile;
use std::{
    collections::{BTreeMap, HashMap},
    path::Path,
    sync::atomic::AtomicBool,
    time::Duration,
};

use crate::probe::Probe;
use std::time::UNIX_EPOCH;

#[tokio::test]
async fn artwork_is_never_shared_between_ambiguous_films() {
    let root = tempfile::tempdir().unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/generated/p1");
    for name in ["a.mp4", "b.mp4"] {
        tokio::fs::copy(fixtures.join("clip-mp4-fr.mp4"), root.path().join(name))
            .await
            .unwrap();
    }
    let bytes = {
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(2, 3)
            .write_to(&mut out, image::ImageFormat::Png)
            .unwrap();
        out.into_inner()
    };
    for name in ["backdrop.png", "a-backdrop.png"] {
        tokio::fs::write(root.path().join(name), &bytes)
            .await
            .unwrap();
    }
    let report = scan(root.path(), &HashMap::new(), &AtomicBool::new(false))
        .await
        .unwrap();
    let a = report
        .files
        .iter()
        .find(|f| f.relative_path == "a.mp4")
        .unwrap();
    let b = report
        .files
        .iter()
        .find(|f| f.relative_path == "b.mp4")
        .unwrap();
    assert_eq!(a.artwork["backdrop"].path, "a-backdrop.png");
    assert!(b.artwork.is_empty());
    tokio::fs::copy(fixtures.join("clip-mkv-fr.mkv"), root.path().join("a.mkv"))
        .await
        .unwrap();
    let report = scan(root.path(), &HashMap::new(), &AtomicBool::new(false))
        .await
        .unwrap();
    assert!(
        report
            .files
            .iter()
            .filter(|f| f.relative_path.starts_with("a."))
            .all(|f| f.artwork.is_empty())
    );
}

#[tokio::test]
async fn single_film_discovers_prefixed_artwork_without_generic_image() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("Film (2024)");
    tokio::fs::create_dir(&dir).await.unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/generated/p1/clip-mp4-fr.mp4");
    tokio::fs::copy(fixture, dir.join("Film.mp4"))
        .await
        .unwrap();
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(2, 3)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    tokio::fs::write(dir.join("Film-background.png"), bytes.into_inner())
        .await
        .unwrap();
    let report = scan(root.path(), &HashMap::new(), &AtomicBool::new(false))
        .await
        .unwrap();
    assert_eq!(report.files.len(), 1);
    assert_eq!(
        report.files[0].artwork["backdrop"].path,
        "Film (2024)/Film-background.png"
    );
}
#[tokio::test]
async fn traversal_refuses_directory_links_and_windows_junctions() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("library");
    let outside = temp.path().join("outside");
    tokio::fs::create_dir(&root).await.unwrap();
    tokio::fs::create_dir(&outside).await.unwrap();
    tokio::fs::write(outside.join("secret.mp4"), b"not media")
        .await
        .unwrap();
    let link = root.join("escape");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, &link).unwrap();
    #[cfg(windows)]
    assert!(
        std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&outside)
            .output()
            .unwrap()
            .status
            .success()
    );
    let report = scan(&root, &HashMap::new(), &AtomicBool::new(false))
        .await
        .unwrap();
    assert_eq!(report.discovered, 0);
    assert_eq!(report.errors.len(), 1);
    assert!(
        confined_file(&root, Path::new("escape/secret.mp4"))
            .await
            .is_err()
    );
    assert!(confined_file(&link, Path::new("secret.mp4")).await.is_err());
    assert!(
        scan(&link, &HashMap::new(), &AtomicBool::new(false))
            .await
            .is_err()
    );
}
#[test]
fn qualified_identity_and_exact_folders_only() {
    let (title, year, ids, key) = identity(Path::new("Film (2024) [tmdb-123]/film-4k.mp4"));
    assert_eq!(title, "Film");
    assert_eq!(year, Some(2024));
    assert_eq!(ids["tmdb"], "123");
    assert_eq!(key, "tmdb:123");
    assert_ne!(
        identity(Path::new("A/Foo.mp4")).3,
        identity(Path::new("B/Foo.mp4")).3
    );
    assert!(metadata_name("Film [12345]").2.is_empty());
    assert_eq!(
        identity(Path::new("Film (2026)/Other Film (2025).mp4")).0,
        "Other Film"
    );
    assert_eq!(
        identity(Path::new("Film (2026) [tmdb-1]/Film [tmdb-2].mp4")).3,
        "tmdb:2"
    );
    assert_ne!(
        identity(Path::new("Film (2026)/Film (2025).mp4")).3,
        "folder:Film (2026)"
    );
    assert!(ignored("Film.trickplay"));
    assert!(ignored("movie.mkv.part"));
}
#[tokio::test]
async fn cached_scan_skips_probe_and_temporary_files() {
    let temp = tempfile::tempdir().unwrap();
    let film = temp.path().join("Film (2024)");
    tokio::fs::create_dir(&film).await.unwrap();
    let path = film.join("film.mp4");
    tokio::fs::write(&path, b"cached synthetic media")
        .await
        .unwrap();
    tokio::fs::write(film.join("incomplete.mkv.part"), b"temporary")
        .await
        .unwrap();
    let m = tokio::fs::metadata(path).await.unwrap();
    let probe = Probe {
        chapters: Vec::new(),
        streams: vec![],
        format: BTreeMap::new(),
    };
    let cache = HashMap::from([(
        "Film (2024)/film.mp4".into(),
        CachedFile {
            content_fingerprint: None,
            size: m.len(),
            modified_ns: m
                .modified()
                .unwrap()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
                .to_string(),
            probe,
        },
    )]);
    tokio::fs::write(
        film.join("movie.nfo"),
        b"<movie><title>Editorial title</title></movie>",
    )
    .await
    .unwrap();
    let result = scan(temp.path(), &cache, &AtomicBool::new(false))
        .await
        .unwrap();
    assert!(result.complete);
    assert_eq!(result.discovered, 1);
    assert_eq!(result.reused, 1);
    assert_eq!(result.probed, 0);
    assert_eq!(result.files[0].title, "Film");
    assert!(
        matches!(&result.files[0].editorial, super::super::nfo::NfoOutcome::Valid { metadata, .. } if metadata.title.as_deref() == Some("Editorial title"))
    );
    tokio::fs::write(
        film.join("movie.nfo"),
        b"<movie><title>Changed</title></movie>",
    )
    .await
    .unwrap();
    let updated = scan(temp.path(), &cache, &AtomicBool::new(false))
        .await
        .unwrap();
    assert_eq!(updated.probed, 0);
    assert!(
        matches!(&updated.files[0].editorial, super::super::nfo::NfoOutcome::Valid { metadata, .. } if metadata.title.as_deref() == Some("Changed"))
    );
    tokio::fs::remove_file(film.join("movie.nfo"))
        .await
        .unwrap();
    let removed = scan(temp.path(), &cache, &AtomicBool::new(false))
        .await
        .unwrap();
    assert_eq!(removed.probed, 0);
    assert!(matches!(
        &removed.files[0].editorial,
        super::super::nfo::NfoOutcome::Missing
    ));
    assert_eq!(removed.files[0].identity_key, result.files[0].identity_key);
    let poster = film.join("poster.png");
    image::DynamicImage::new_rgb8(2, 3).save(&poster).unwrap();
    let with_image = scan(temp.path(), &cache, &AtomicBool::new(false))
        .await
        .unwrap();
    assert_eq!(with_image.probed, 0);
    let image = with_image.files[0].poster.clone().unwrap();
    let image_cache = HashMap::from([(image.path.clone(), image.clone())]);
    let reused = scan_with_images(temp.path(), &cache, &image_cache, &AtomicBool::new(false))
        .await
        .unwrap();
    assert_eq!(
        reused.files[0].poster.as_ref().unwrap().modified_ns,
        image.modified_ns
    );
    image::DynamicImage::new_rgb8(5, 7).save(&poster).unwrap();
    let updated = scan_with_images(temp.path(), &cache, &image_cache, &AtomicBool::new(false))
        .await
        .unwrap();
    assert_eq!(updated.probed, 0);
    assert!(updated.files[0].poster.is_some());
    assert_ne!(
        updated.files[0].poster.as_ref().unwrap().size_bytes,
        image.size_bytes
    );
    tokio::fs::remove_file(&poster).await.unwrap();
    let deleted = scan_with_images(temp.path(), &cache, &image_cache, &AtomicBool::new(false))
        .await
        .unwrap();
    assert_eq!(deleted.probed, 0);
    assert!(deleted.files[0].poster.is_none());
    assert_eq!(deleted.files[0].identity_key, result.files[0].identity_key);
    assert!(
        confined_file(temp.path(), Path::new("../outside.mp4"))
            .await
            .is_err()
    );
    let cancelled = scan(temp.path(), &cache, &AtomicBool::new(true))
        .await
        .unwrap();
    assert!(!cancelled.complete);
}

#[tokio::test]
async fn dense_directory_scan_caches_folder_videos_and_reuses_sidecars() {
    let temp = tempfile::tempdir().unwrap();
    let dense = temp.path().join("Dense [tmdb-777]");
    tokio::fs::create_dir(&dense).await.unwrap();
    tokio::fs::write(
        dense.join("movie.nfo"),
        b"<movie><title>Dense Collection</title><year>2025</year></movie>",
    )
    .await
    .unwrap();
    let poster_path = dense.join("poster.png");
    image::DynamicImage::new_rgb8(2, 2)
        .save(&poster_path)
        .unwrap();

    let mut cache = HashMap::new();
    for i in 0..20 {
        let filename = format!("version-{i}.mp4");
        let file_path = dense.join(&filename);
        tokio::fs::write(&file_path, b"synthetic-video-payload")
            .await
            .unwrap();
        let meta = tokio::fs::metadata(&file_path).await.unwrap();
        let modified_ns = meta
            .modified()
            .unwrap()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
            .to_string();
        cache.insert(
            format!("Dense [tmdb-777]/{filename}"),
            CachedFile {
                content_fingerprint: None,
                size: meta.len(),
                modified_ns,
                probe: Probe {
                    chapters: Vec::new(),
                    streams: vec![],
                    format: BTreeMap::new(),
                },
            },
        );
    }

    let first = scan(temp.path(), &cache, &AtomicBool::new(false))
        .await
        .unwrap();
    assert!(first.complete);
    assert_eq!(first.discovered, 20);
    assert_eq!(first.reused, 20);
    assert_eq!(first.probed, 0);
    assert!(first.errors.is_empty());
    assert!(first.warnings.is_empty());
    let poster = first.files[0].poster.clone().unwrap();
    let image_cache = HashMap::from([(poster.path.clone(), poster)]);

    let start = std::time::Instant::now();
    let second = scan_with_images(temp.path(), &cache, &image_cache, &AtomicBool::new(false))
        .await
        .unwrap();
    let elapsed = start.elapsed();
    assert!(second.complete);
    assert_eq!(second.discovered, 20);
    assert_eq!(second.reused, 20);
    assert_eq!(second.probed, 0);
    assert!(elapsed < Duration::from_secs(2));
}

#[tokio::test]
async fn scanner_ignores_nas_system_folders_nomedia_and_custom_exclusions() {
    let temp = tempfile::tempdir().unwrap();
    // 1. Valid video
    let valid_dir = temp.path().join("Valid (2024)");
    tokio::fs::create_dir(&valid_dir).await.unwrap();
    let path = valid_dir.join("valid.mp4");
    tokio::fs::write(&path, b"synthetic media").await.unwrap();
    let m = tokio::fs::metadata(&path).await.unwrap();
    let cache = HashMap::from([(
        "Valid (2024)/valid.mp4".into(),
        CachedFile {
            content_fingerprint: None,
            size: m.len(),
            modified_ns: m
                .modified()
                .unwrap()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
                .to_string(),
            probe: Probe::default(),
        },
    )]);

    // 2. Synology #recycle folder
    let recycle = temp.path().join("#recycle");
    tokio::fs::create_dir(&recycle).await.unwrap();
    tokio::fs::write(recycle.join("deleted.mp4"), b"deleted media")
        .await
        .unwrap();

    // 3. Synology @eaDir folder
    let eadir = valid_dir.join("@eaDir");
    tokio::fs::create_dir(&eadir).await.unwrap();
    tokio::fs::write(eadir.join("thumb.mp4"), b"thumb media")
        .await
        .unwrap();

    // 4. Directory with .nomedia file
    let nomedia_dir = temp.path().join("IgnoredFolder");
    tokio::fs::create_dir(&nomedia_dir).await.unwrap();
    tokio::fs::write(nomedia_dir.join(".nomedia"), b"")
        .await
        .unwrap();
    tokio::fs::write(nomedia_dir.join("unwanted.mp4"), b"unwanted")
        .await
        .unwrap();

    // 5. Custom excluded folder
    let bonus_dir = temp.path().join("Bonus");
    tokio::fs::create_dir(&bonus_dir).await.unwrap();
    tokio::fs::write(bonus_dir.join("featurette.mp4"), b"bonus")
        .await
        .unwrap();

    let excluded = vec!["bonus".to_string()];
    let mut files = Vec::new();
    let report = scan_streaming(
        temp.path(),
        "films",
        &cache,
        &HashMap::new(),
        &AtomicBool::new(false),
        100,
        &excluded,
        |batch| {
            files.extend(batch.files);
            async { Ok(()) }
        },
    )
    .await
    .unwrap();

    assert_eq!(report.discovered, 1);
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].title, "Valid");
}

#[test]
fn identity_handles_movies_in_folders_with_and_without_years() {
    // 1. Generic filename inside a folder without year
    let (title, year, ids, group) = identity(Path::new("Inception/movie.mkv"));
    assert_eq!(title, "Inception");
    assert_eq!(year, None);
    assert!(ids.is_empty());
    assert_eq!(group, "folder:Inception");

    // 2. Matching filename inside a folder without year
    let (title, year, _ids, group) = identity(Path::new("Inception/Inception.mkv"));
    assert_eq!(title, "Inception");
    assert_eq!(year, None);
    assert_eq!(group, "folder:Inception");

    // 3. Multi-part movie in folder without year
    let (t1, _, _, g1) = identity(Path::new("Fight Club/cd1.mkv"));
    let (t2, _, _, g2) = identity(Path::new("Fight Club/cd2.mkv"));
    assert_eq!(t1, "Fight Club");
    assert_eq!(t2, "Fight Club");
    assert_eq!(g1, "folder:Fight Club");
    assert_eq!(g2, "folder:Fight Club");

    // 4. Movie in folder with year in folder name
    let (title, year, _, group) = identity(Path::new("Inception (2010)/movie.mkv"));
    assert_eq!(title, "Inception");
    assert_eq!(year, Some(2010));
    assert_eq!(group, "folder:Inception (2010)");

    // 5. Direct root movie file
    let (title, year, _, group) = identity(Path::new("Interstellar.mkv"));
    assert_eq!(title, "Interstellar");
    assert_eq!(year, None);
    assert_eq!(group, "file:Interstellar.mkv");

    // 6. Generic category folder with distinct movies
    let (t1, _, _, g1) = identity(Path::new("Action/Matrix.mkv"));
    let (t2, _, _, g2) = identity(Path::new("Action/Die Hard.mkv"));
    assert_eq!(t1, "Matrix");
    assert_eq!(t2, "Die Hard");
    assert_eq!(g1, "file:Action/Matrix.mkv");
    assert_eq!(g2, "file:Action/Die Hard.mkv");
}

#[test]
fn parse_series_episode_fallback_for_movies_and_non_standard_folders() {
    // 1. Movie folder inside a series library
    let parsed = parse_series_episode(Path::new("El Camino/El Camino.mkv")).unwrap();
    assert_eq!(parsed.series_raw_title, "El Camino");
    assert_eq!(parsed.season_number, 1);
    assert_eq!(parsed.episode_number, 1);
    assert_eq!(parsed.episode_title, "El Camino");

    // 2. Generic movie filename inside a folder
    let parsed = parse_series_episode(Path::new("Special Show/movie.mp4")).unwrap();
    assert_eq!(parsed.series_raw_title, "Special Show");
    assert_eq!(parsed.season_number, 1);
    assert_eq!(parsed.episode_number, 1);
    assert_eq!(parsed.episode_title, "Special Show");

    // 3. Numbered episodes without "Season" folder or SxxExx
    let parsed = parse_series_episode(Path::new("Band of Brothers/01 - Currahee.mkv")).unwrap();
    assert_eq!(parsed.series_raw_title, "Band of Brothers");
    assert_eq!(parsed.season_number, 1);
    assert_eq!(parsed.episode_number, 1);
    assert_eq!(parsed.episode_title, "Currahee");

    // 4. Standard SxxExx pattern still works
    let parsed = parse_series_episode(Path::new("Breaking Bad/Season 1/S01E02.mkv")).unwrap();
    assert_eq!(parsed.series_raw_title, "Breaking Bad");
    assert_eq!(parsed.season_number, 1);
    assert_eq!(parsed.episode_number, 2);
}

#[test]
fn clean_filename_strips_release_tags_and_parses_unparenthesized_year() {
    let (title, year, ids) = metadata_name("Mon.Film.2024.1080p.WEBRip.x264");
    assert_eq!(title, "Mon Film");
    assert_eq!(year, Some(2024));
    assert!(ids.is_empty());

    let (title, year, _) = metadata_name("Crouching.Tiger.Hidden.Dragon.4k");
    assert_eq!(title, "Crouching Tiger Hidden Dragon");
    assert_eq!(year, None);

    let (title, year, ids) = metadata_name("Super.Movie.1999.720p.BluRay.x264 [tmdb-12345]");
    assert_eq!(title, "Super Movie");
    assert_eq!(year, Some(1999));
    assert_eq!(ids.get("tmdb").map(String::as_str), Some("12345"));

    let (title, year, ids) = metadata_name("Movie.Title.2021.1080p [tmdbid=54321]");
    assert_eq!(title, "Movie Title");
    assert_eq!(year, Some(2021));
    assert_eq!(ids.get("tmdb").map(String::as_str), Some("54321"));

    let (title, year, _) = metadata_name("[HorribleSubs] Made in Abyss - 13 [720p]");
    assert_eq!(title, "Made in Abyss - 13");
    assert_eq!(year, None);

    let (title, year, ids) = metadata_name("Gladiator.[2000].1080p.mkv");
    assert_eq!(title, "Gladiator");
    assert_eq!(year, Some(2000));
    assert!(ids.is_empty());

    let (title, year, ids) = metadata_name("Another.Movie [tmdb-1999]");
    assert_eq!(title, "Another Movie");
    assert_eq!(year, None);
    assert_eq!(ids.get("tmdb").map(String::as_str), Some("1999"));

    let ep = parse_series_episode(Path::new(
        "Show/Season 1/Show.S01E01.Pilot.1080p.BluRay.x264.mkv",
    ))
    .unwrap();
    assert_eq!(ep.episode_title, "Pilot");

    let ep =
        parse_series_episode(Path::new("Show/Season 1/Show.S01E01.1080p.BluRay.x264.mkv")).unwrap();
    assert_eq!(ep.episode_title, "S01E01");
}

/// An unreadable subfolder (Docker UID mismatch, NAS ACL) is a warning: the traversal finishes,
/// publication stays partial (`complete == false`) and the scan is not interrupted or failed.
#[cfg(unix)]
#[tokio::test]
async fn unreadable_subfolder_is_a_warning_not_an_interruption() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let locked = root.path().join("locked");
    tokio::fs::create_dir(&locked).await.unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::read_dir(&locked).is_ok() {
        // Running as root: permissions are not enforced, nothing to verify.
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).unwrap();
        return;
    }
    let report = scan(root.path(), &HashMap::new(), &AtomicBool::new(false))
        .await
        .unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert!(
        !report.complete,
        "missing files must not be marked unavailable"
    );
    assert!(!report.interrupted);
    assert_eq!(report.errors_count, 0);
    assert_eq!(report.warnings_count, 1);
    assert_eq!(report.warnings[0].relative_path, "locked");
}

#[tokio::test]
async fn cancelled_traversal_is_interrupted() {
    let root = tempfile::tempdir().unwrap();
    tokio::fs::create_dir(root.path().join("folder"))
        .await
        .unwrap();
    let report = scan(root.path(), &HashMap::new(), &AtomicBool::new(true))
        .await
        .unwrap();
    assert!(!report.complete);
    assert!(report.interrupted);
}

/// Everything observable about a tree: kind, size, mtime and permissions of every entry.
type TreeSnapshot = BTreeMap<String, (bool, u64, Option<std::time::SystemTime>, bool)>;

fn snapshot(root: &Path) -> TreeSnapshot {
    fn walk(dir: &Path, root: &Path, out: &mut TreeSnapshot) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let meta = entry.metadata().unwrap();
            let rel = entry
                .path()
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            out.insert(
                rel,
                (
                    meta.is_dir(),
                    meta.len(),
                    meta.modified().ok(),
                    meta.permissions().readonly(),
                ),
            );
            if meta.is_dir() {
                walk(&entry.path(), root, out);
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

fn cached(path: &Path) -> CachedFile {
    let m = std::fs::metadata(path).unwrap();
    CachedFile {
        content_fingerprint: None,
        size: m.len(),
        modified_ns: m
            .modified()
            .unwrap()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
            .to_string(),
        probe: Probe {
            chapters: Vec::new(),
            streams: vec![],
            format: BTreeMap::new(),
        },
    }
}

#[cfg(unix)]
fn set_modes(root: &Path, dir_mode: u32, file_mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    fn walk(dir: &Path, dir_mode: u32, file_mode: u32) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, dir_mode, file_mode);
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(dir_mode)).unwrap();
            } else {
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(file_mode))
                    .unwrap();
            }
        }
    }
    walk(root, dir_mode, file_mode);
    std::fs::set_permissions(root, std::fs::Permissions::from_mode(dir_mode)).unwrap();
}

#[tokio::test]
async fn scanning_a_library_never_writes_beside_the_media() {
    let temp = tempfile::tempdir().unwrap();
    let library = temp.path().join("library");
    let film = library.join("Film (2024)");
    tokio::fs::create_dir_all(&film).await.unwrap();
    tokio::fs::write(film.join("film.mp4"), vec![7u8; 300_000])
        .await
        .unwrap();
    tokio::fs::write(
        film.join("movie.nfo"),
        b"<movie><title>T</title><tmdbid>5</tmdbid></movie>",
    )
    .await
    .unwrap();
    tokio::fs::write(film.join("poster.jpg"), b"not really an image")
        .await
        .unwrap();
    let cache = HashMap::from([(
        "Film (2024)/film.mp4".to_owned(),
        cached(&film.join("film.mp4")),
    )]);
    #[cfg(unix)]
    set_modes(&library, 0o555, 0o444);
    let before = snapshot(&library);
    let cold = scan(&library, &HashMap::new(), &AtomicBool::new(false)).await;
    let warm = scan(&library, &cache, &AtomicBool::new(false)).await;
    let after = snapshot(&library);
    #[cfg(unix)]
    set_modes(&library, 0o755, 0o644);
    let (cold, warm) = (cold.unwrap(), warm.unwrap());
    assert_eq!(
        before, after,
        "a scan must leave the library exactly as it found it"
    );
    assert!(
        after.keys().all(|p| !p.contains(".aether")),
        "no .aether folder or file may appear beside the media: {after:?}"
    );
    // The cold scan probes (and fails on synthetic bytes); the warm one runs the same read-only
    // path through NFO, artwork and fingerprint discovery.
    assert_eq!(cold.discovered, 1);
    assert_eq!(warm.discovered, 1);
    assert_eq!(warm.files.len(), 1);
}

#[tokio::test]
async fn content_fingerprint_is_computed_once_then_reused_until_the_file_changes() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("Film (2024).mp4");
    tokio::fs::write(&path, vec![9u8; 400_000]).await.unwrap();
    let entry = cached(&path);
    let one = |entry: CachedFile| HashMap::from([("Film (2024).mp4".to_owned(), entry)]);

    let fresh = scan(temp.path(), &one(entry.clone()), &AtomicBool::new(false))
        .await
        .unwrap();
    let computed = fresh.files[0]
        .content_fingerprint
        .clone()
        .expect("unknown to the Core: fingerprinted");
    assert!(crate::fingerprint::is_well_formed(&computed));
    assert!(computed.starts_with("b3s1:400000:"));

    // The Core already knows a value for this exact size and mtime: it is reused verbatim (a
    // sentinel proves the file was not read again, which keeps a Node restart cheap).
    let sentinel = "b3s1:400000:00000000000000000000000000000000".to_owned();
    let mut known = entry.clone();
    known.content_fingerprint = Some(sentinel.clone());
    let reused = scan(temp.path(), &one(known.clone()), &AtomicBool::new(false))
        .await
        .unwrap();
    assert_eq!(reused.reused, 1);
    assert_eq!(
        reused.files[0].content_fingerprint.as_deref(),
        Some(sentinel.as_str())
    );

    // A malformed cached value is ignored, not trusted.
    known.content_fingerprint = Some("garbage".into());
    let repaired = scan(temp.path(), &one(known), &AtomicBool::new(false))
        .await
        .unwrap();
    assert_eq!(
        repaired.files[0].content_fingerprint.as_deref(),
        Some(computed.as_str())
    );

    // The file changes: the stale cached value no longer applies, even though it is well formed.
    tokio::fs::write(&path, vec![9u8; 400_001]).await.unwrap();
    let mut stale = entry;
    stale.content_fingerprint = Some(sentinel.clone());
    let changed = scan(temp.path(), &one(stale), &AtomicBool::new(false))
        .await
        .unwrap();
    assert!(
        changed
            .files
            .iter()
            .all(|f| f.content_fingerprint.as_deref() != Some(sentinel.as_str()))
    );
}

#[tokio::test]
async fn contradictory_nfo_identifiers_warn_and_are_not_used() {
    let temp = tempfile::tempdir().unwrap();
    let film = temp.path().join("Dense [tmdb-777] (2024)");
    tokio::fs::create_dir(&film).await.unwrap();
    let path = film.join("film.mp4");
    tokio::fs::write(&path, b"synthetic").await.unwrap();
    // The NFO disagrees with the folder name (and so does its own second source).
    tokio::fs::write(
        film.join("movie.nfo"),
        b"<movie><title>Dense</title><tmdbid>1</tmdbid><imdbid>tt0000001</imdbid></movie>",
    )
    .await
    .unwrap();
    let cache = HashMap::from([("Dense [tmdb-777] (2024)/film.mp4".to_owned(), cached(&path))]);
    let report = scan(temp.path(), &cache, &AtomicBool::new(false))
        .await
        .unwrap();
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.message.contains("contradictory tmdb")),
        "{:?}",
        report.warnings
    );
    match &report.files[0].editorial {
        super::super::nfo::NfoOutcome::Valid { metadata, .. } => {
            assert_eq!(metadata.title.as_deref(), Some("Dense"), "the rest is kept");
            assert_eq!(metadata.external_ids["imdb"], "tt0000001");
        }
        other => panic!("expected a valid NFO, got {other:?}"),
    }
    // The name keeps its own id for matching; the Core drops the contradicted provider.
    assert_eq!(report.files[0].external_ids["tmdb"], "777");
}
