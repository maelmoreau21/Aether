//! Renamed / moved films keep their work; ambiguous or cross-library cases never merge anything.
//! Synthetic staged scans only (no media fixtures): the Core decides from what Nodes report.

use media_contracts::media::FilmMetadata;
use media_poc::{
    database::{query, query_scalar},
    identity::IdentityStore,
    inventory::{InventoryBatch, NfoOutcome, ScannedFile, publish_staged_scan, stage_batch},
};
use std::{collections::BTreeMap, path::Path};

const FP_A: &str = "b3s1:1000:00112233445566778899aabbccddeeff";
const FP_B: &str = "b3s1:2000:ffeeddccbbaa99887766554433221100";

async fn store_with_libraries(root: &Path, libraries: &[&str]) -> IdentityStore {
    let store = IdentityStore::open(&root.join("relocation.sqlite"))
        .await
        .unwrap();
    for id in libraries {
        let dir = root.join(id);
        std::fs::create_dir_all(&dir).unwrap();
        query("INSERT INTO libraries(id, name, kind, node_id, root, enabled) VALUES(?, ?, 'films', 'local', ?, 1)")
            .bind(*id)
            .bind(*id)
            .bind(dir.to_string_lossy().as_ref())
            .execute(store.pool())
            .await
            .unwrap();
    }
    store
}

struct Film {
    path: &'static str,
    folder_key: &'static str,
    size: u64,
    mtime: &'static str,
    fingerprint: Option<&'static str>,
    ids: &'static [(&'static str, &'static str)],
    nfo_ids: &'static [(&'static str, &'static str)],
}
impl Film {
    fn new(path: &'static str, folder_key: &'static str, fingerprint: &'static str) -> Self {
        Self {
            path,
            folder_key,
            size: 1000,
            mtime: "500",
            fingerprint: Some(fingerprint),
            ids: &[],
            nfo_ids: &[],
        }
    }
    fn scanned(&self) -> ScannedFile {
        let map = |pairs: &[(&str, &str)]| -> BTreeMap<String, String> {
            pairs
                .iter()
                .map(|(p, v)| ((*p).to_owned(), (*v).to_owned()))
                .collect()
        };
        ScannedFile {
            relative_path: self.path.into(),
            size: self.size,
            modified_ns: self.mtime.into(),
            title: "Film".into(),
            year: Some(2020),
            identity_key: self.folder_key.into(),
            external_ids: map(self.ids),
            content_fingerprint: self.fingerprint.map(Into::into),
            mime: "video/x-matroska".into(),
            probe: serde_json::from_value(serde_json::json!({
                "format": {"duration": "60.0"},
                "streams": [{"index": 0, "codec_type": "video", "codec_name": "h264"}]
            }))
            .unwrap(),
            editorial: if self.nfo_ids.is_empty() {
                NfoOutcome::Missing
            } else {
                NfoOutcome::Valid {
                    metadata: FilmMetadata {
                        title: Some("Film".into()),
                        external_ids: map(self.nfo_ids),
                        ..Default::default()
                    },
                    path: "movie.nfo".into(),
                    priority: 1,
                }
            },
            ..Default::default()
        }
    }
}

async fn scan(
    store: &IdentityStore,
    scan_id: &str,
    library: &str,
    root: &Path,
    films: &[Film],
    complete: bool,
) {
    scan_files(
        store,
        scan_id,
        library,
        root,
        films.iter().map(Film::scanned).collect(),
        complete,
    )
    .await;
}

async fn scan_files(
    store: &IdentityStore,
    scan_id: &str,
    library: &str,
    root: &Path,
    files: Vec<ScannedFile>,
    complete: bool,
) {
    stage_batch(
        store,
        scan_id,
        library,
        &InventoryBatch {
            scan_id: scan_id.into(),
            sequence: 0,
            files,
            errors: vec![],
            warnings: vec![],
        },
    )
    .await
    .unwrap();
    publish_staged_scan(store, scan_id, library, &root.join(library), complete)
        .await
        .unwrap();
}

async fn count(store: &IdentityStore, sql: &'static str, library: &str) -> i64 {
    query_scalar::<_, i64>(sql)
        .bind(library)
        .fetch_one(store.pool())
        .await
        .unwrap()
}
async fn works(store: &IdentityStore, library: &str) -> i64 {
    count(
        store,
        "SELECT COUNT(*) FROM inventory_works WHERE library_id=?",
        library,
    )
    .await
}
async fn assets(store: &IdentityStore, library: &str) -> i64 {
    count(
        store,
        "SELECT COUNT(*) FROM inventory_assets WHERE library_id=?",
        library,
    )
    .await
}
async fn only_asset(store: &IdentityStore, library: &str) -> (String, String, i64) {
    let row: (String, String, i64) = sqlx::query_as(
        "SELECT a.id, a.relative_path, a.available FROM inventory_assets a WHERE a.library_id=?",
    )
    .bind(library)
    .fetch_one(store.pool())
    .await
    .unwrap();
    row
}
async fn work_of(store: &IdentityStore, asset: &str) -> String {
    query_scalar("SELECT e.work_id FROM inventory_assets a JOIN inventory_sources s ON s.id=a.source_id JOIN inventory_versions v ON v.id=s.version_id JOIN inventory_editions e ON e.id=v.edition_id WHERE a.id=?")
        .bind(asset)
        .fetch_one(store.pool())
        .await
        .unwrap()
}

#[tokio::test]
async fn renamed_and_moved_film_keeps_work_corrections_and_asset() {
    let temp = tempfile::tempdir().unwrap();
    let store = store_with_libraries(temp.path(), &["films"]).await;
    let root = temp.path();
    scan(
        &store,
        "s1",
        "films",
        root,
        &[Film::new("Old (2020)/Film.mkv", "folder:Old (2020)", FP_A)],
        true,
    )
    .await;
    let (asset, _, _) = only_asset(&store, "films").await;
    let work = work_of(&store, &asset).await;
    // A hand-written French title, plus a prepared-cache style row keyed by the asset id.
    query("INSERT INTO work_translations(work_id,locale,language,source,priority,title,updated_at) VALUES(?,'fr-FR','fr','manual',100,'Titre corrigé à la main',1)")
        .bind(&work)
        .execute(store.pool())
        .await
        .unwrap();

    // The folder and the file are both renamed.
    scan(
        &store,
        "s2",
        "films",
        root,
        &[Film::new(
            "New Name (2020)/Film 1080p.mkv",
            "folder:New Name (2020)",
            FP_A,
        )],
        true,
    )
    .await;

    assert_eq!(assets(&store, "films").await, 1, "no second asset");
    assert_eq!(works(&store, "films").await, 1, "no second work");
    let (after_asset, path, available) = only_asset(&store, "films").await;
    assert_eq!(
        after_asset, asset,
        "the asset id (and every cache keyed by it) is kept"
    );
    assert_eq!(path, "New Name (2020)/Film 1080p.mkv");
    assert_eq!(available, 1);
    assert_eq!(work_of(&store, &asset).await, work);
    let key: String = query_scalar("SELECT identity_key FROM inventory_works WHERE id=?")
        .bind(&work)
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert!(key.ends_with("\nfolder:New Name (2020)"), "{key}");
    let manual: String =
        query_scalar("SELECT title FROM work_translations WHERE work_id=? AND source='manual'")
            .bind(&work)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(manual, "Titre corrigé à la main");

    // The hint is remembered for the next scan, and an unchanged third scan changes nothing.
    scan(
        &store,
        "s3",
        "films",
        root,
        &[Film::new(
            "New Name (2020)/Film 1080p.mkv",
            "folder:New Name (2020)",
            FP_A,
        )],
        true,
    )
    .await;
    assert_eq!(
        (works(&store, "films").await, assets(&store, "films").await),
        (1, 1)
    );
    let remembered: String =
        query_scalar("SELECT fingerprint FROM inventory_asset_fingerprints WHERE asset_id=?")
            .bind(&asset)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(remembered, FP_A);
}

#[tokio::test]
async fn copies_modified_files_and_conflicting_ids_are_never_treated_as_moves() {
    let temp = tempfile::tempdir().unwrap();
    let store = store_with_libraries(temp.path(), &["films"]).await;
    let root = temp.path();
    let original = Film::new("A (2020)/Film.mkv", "folder:A (2020)", FP_A);
    scan(
        &store,
        "s1",
        "films",
        root,
        &[Film::new("A (2020)/Film.mkv", "folder:A (2020)", FP_A)],
        true,
    )
    .await;

    // A copy appears next to the original, which is still there: a duplicate, not a move.
    scan(
        &store,
        "s2",
        "films",
        root,
        &[
            original,
            Film::new("B (2020)/Film.mkv", "folder:B (2020)", FP_A),
        ],
        true,
    )
    .await;
    assert_eq!(
        (works(&store, "films").await, assets(&store, "films").await),
        (2, 2)
    );
    let alive: i64 = count(
        &store,
        "SELECT COUNT(*) FROM inventory_assets WHERE library_id=? AND available=1",
        "films",
    )
    .await;
    assert_eq!(alive, 2);

    // Different content under a new name (nothing corroborates it): a new work, the old asset
    // is kept as unavailable rather than being hijacked.
    let temp2 = tempfile::tempdir().unwrap();
    let store2 = store_with_libraries(temp2.path(), &["films"]).await;
    scan(
        &store2,
        "s1",
        "films",
        temp2.path(),
        &[Film::new("A (2020)/Film.mkv", "folder:A (2020)", FP_A)],
        true,
    )
    .await;
    let (old_asset, _, _) = only_asset(&store2, "films").await;
    let mut different = Film::new("Other (2021)/Another.mkv", "folder:Other (2021)", FP_A);
    different.mtime = "999"; // same size and sampled bytes, but nothing else agrees
    scan(&store2, "s2", "films", temp2.path(), &[different], true).await;
    assert_eq!(
        (
            works(&store2, "films").await,
            assets(&store2, "films").await
        ),
        (2, 2)
    );
    let old: (String, i64) =
        sqlx::query_as("SELECT relative_path, available FROM inventory_assets WHERE id=?")
            .bind(&old_asset)
            .fetch_one(store2.pool())
            .await
            .unwrap();
    assert_eq!(old, ("A (2020)/Film.mkv".to_owned(), 0));

    // Contradicting identifiers veto a content match even when everything else agrees.
    let temp3 = tempfile::tempdir().unwrap();
    let store3 = store_with_libraries(temp3.path(), &["films"]).await;
    let mut first = Film::new("A (2020)/Film.mkv", "folder:A (2020)", FP_A);
    first.nfo_ids = &[("tmdb", "1")];
    scan(&store3, "s1", "films", temp3.path(), &[first], true).await;
    let mut second = Film::new("B (2020)/Film.mkv", "folder:B (2020)", FP_A);
    second.nfo_ids = &[("tmdb", "2")];
    scan(&store3, "s2", "films", temp3.path(), &[second], true).await;
    assert_eq!(
        (
            works(&store3, "films").await,
            assets(&store3, "films").await
        ),
        (2, 2)
    );
}

#[tokio::test]
async fn reencoded_upgrade_follows_a_reliable_id_and_an_incomplete_scan_follows_nothing() {
    let temp = tempfile::tempdir().unwrap();
    let store = store_with_libraries(temp.path(), &["films"]).await;
    let root = temp.path();
    let mut v1 = Film::new("Heat (1995)/Heat.720p.mkv", "folder:Heat (1995)", FP_A);
    v1.nfo_ids = &[("tmdb", "949"), ("imdb", "tt0113277")];
    scan(&store, "s1", "films", root, &[v1], true).await;
    let (asset, _, _) = only_asset(&store, "films").await;
    let work = work_of(&store, &asset).await;
    let stored: String = query_scalar("SELECT external_ids_json FROM inventory_works WHERE id=?")
        .bind(&work)
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert!(stored.contains("949"), "{stored}");

    // The same film re-downloaded as a 1080p encode in another folder: other bytes, other size
    // and mtime, same NFO identifiers. An incomplete scan must not move anything...
    let mut v2 = Film::new(
        "Heat (1995) [1080p]/Heat.1080p.mkv",
        "folder:Heat (1995) [1080p]",
        FP_B,
    );
    v2.size = 2000;
    v2.mtime = "900";
    v2.nfo_ids = &[("tmdb", "949"), ("imdb", "tt0113277")];
    scan(&store, "s2", "films", root, &[v2], false).await;
    assert_eq!(
        works(&store, "films").await,
        2,
        "incomplete scan: unreadable is not vanished"
    );

    // ...while a complete one does. (Fresh store: the previous scan already created a work.)
    let temp2 = tempfile::tempdir().unwrap();
    let store2 = store_with_libraries(temp2.path(), &["films"]).await;
    let mut a = Film::new("Heat (1995)/Heat.720p.mkv", "folder:Heat (1995)", FP_A);
    a.nfo_ids = &[("tmdb", "949")];
    scan(&store2, "s1", "films", temp2.path(), &[a], true).await;
    let (asset2, _, _) = only_asset(&store2, "films").await;
    let work2 = work_of(&store2, &asset2).await;
    let mut b = Film::new(
        "Heat (1995) [1080p]/Heat.1080p.mkv",
        "folder:Heat (1995) [1080p]",
        FP_B,
    );
    b.size = 2000;
    b.mtime = "900";
    b.nfo_ids = &[("tmdb", "949")];
    scan(&store2, "s2", "films", temp2.path(), &[b], true).await;
    assert_eq!(
        (
            works(&store2, "films").await,
            assets(&store2, "films").await
        ),
        (1, 1)
    );
    let (kept, path, available) = only_asset(&store2, "films").await;
    assert_eq!(kept, asset2);
    assert_eq!(path, "Heat (1995) [1080p]/Heat.1080p.mkv");
    assert_eq!(available, 1);
    assert_eq!(work_of(&store2, &kept).await, work2);
    let size: i64 = query_scalar("SELECT size FROM inventory_assets WHERE id=?")
        .bind(&kept)
        .fetch_one(store2.pool())
        .await
        .unwrap();
    assert_eq!(size, 2000, "the record now describes the new file");
}

#[tokio::test]
async fn libraries_are_never_merged_even_with_identical_files() {
    let temp = tempfile::tempdir().unwrap();
    let store = store_with_libraries(temp.path(), &["lib-a", "lib-b"]).await;
    let root = temp.path();
    scan(
        &store,
        "s1",
        "lib-a",
        root,
        &[Film::new("Old (2020)/Film.mkv", "folder:Old (2020)", FP_A)],
        true,
    )
    .await;
    let (asset_a, _, _) = only_asset(&store, "lib-a").await;
    // The same bytes, name and mtime show up in another library (other permissions) while the
    // first library's file vanishes: still two independent libraries, nothing moves across.
    scan(
        &store,
        "s2",
        "lib-b",
        root,
        &[Film::new("Old (2020)/Film.mkv", "folder:Old (2020)", FP_A)],
        true,
    )
    .await;
    scan(&store, "s3", "lib-a", root, &[], true).await;
    assert_eq!(
        (works(&store, "lib-a").await, assets(&store, "lib-a").await),
        (1, 1)
    );
    assert_eq!(
        (works(&store, "lib-b").await, assets(&store, "lib-b").await),
        (1, 1)
    );
    let (still_a, _, available) = only_asset(&store, "lib-a").await;
    assert_eq!(still_a, asset_a);
    assert_eq!(
        available, 0,
        "the vanished file stays unavailable in its own library"
    );
    let (asset_b, _, available_b) = only_asset(&store, "lib-b").await;
    assert_ne!(asset_b, asset_a);
    assert_eq!(available_b, 1);
    assert_ne!(
        work_of(&store, &asset_a).await,
        work_of(&store, &asset_b).await
    );
}

// ---------------------------------------------------------------------------------------------
// Series: the episode keeps its season/episode, the series follows its folder.
// ---------------------------------------------------------------------------------------------

async fn series_store(root: &Path, library: &str) -> IdentityStore {
    let store = store_with_libraries(root, &[library]).await;
    query("UPDATE libraries SET kind='series' WHERE id=?")
        .bind(library)
        .execute(store.pool())
        .await
        .unwrap();
    store
}

fn episode(
    path: &str,
    series_key: &str,
    season: u32,
    number: u32,
    fingerprint: &str,
) -> ScannedFile {
    ScannedFile {
        relative_path: path.into(),
        size: 1000,
        modified_ns: "500".into(),
        title: format!("Episode {number}"),
        year: Some(2020),
        identity_key: format!("episode:{series_key}:s{season:02}e{number:02}"),
        content_fingerprint: Some(fingerprint.into()),
        mime: "video/x-matroska".into(),
        probe: serde_json::from_value(serde_json::json!({
            "format": {"duration": "60.0"},
            "streams": [{"index": 0, "codec_type": "video", "codec_name": "h264"}]
        }))
        .unwrap(),
        series_identity_key: Some(series_key.into()),
        series_title: Some("Show".into()),
        series_year: Some(2020),
        series_external_ids: Some(BTreeMap::new()),
        season_number: Some(season),
        episode_number: Some(number),
        ..Default::default()
    }
}

const FP_E1: &str = "b3s1:1000:11111111111111111111111111111111";
const FP_E2: &str = "b3s1:1000:22222222222222222222222222222222";

async fn series_count(store: &IdentityStore, library: &str) -> i64 {
    count(
        store,
        "SELECT COUNT(*) FROM inventory_works WHERE library_id=? AND kind='series'",
        library,
    )
    .await
}
async fn episodes_count(store: &IdentityStore, library: &str) -> i64 {
    count(
        store,
        "SELECT COUNT(*) FROM inventory_works WHERE library_id=? AND kind='episode'",
        library,
    )
    .await
}
async fn asset_at(store: &IdentityStore, library: &str, path: &str) -> Option<String> {
    query_scalar("SELECT id FROM inventory_assets WHERE library_id=? AND relative_path=?")
        .bind(library)
        .bind(path)
        .fetch_optional(store.pool())
        .await
        .unwrap()
}
async fn series_of(store: &IdentityStore, asset: &str) -> String {
    query_scalar("SELECT ss.series_id FROM inventory_assets a JOIN inventory_sources s ON s.id=a.source_id JOIN inventory_versions v ON v.id=s.version_id JOIN inventory_editions e ON e.id=v.edition_id JOIN inventory_episodes ie ON ie.work_id=e.work_id JOIN inventory_seasons ss ON ss.id=ie.season_id WHERE a.id=?")
        .bind(asset)
        .fetch_one(store.pool())
        .await
        .unwrap()
}

#[tokio::test]
async fn renamed_series_folder_keeps_series_season_episode_and_corrections() {
    let temp = tempfile::tempdir().unwrap();
    let store = series_store(temp.path(), "shows").await;
    let root = temp.path();
    let old = "folder:Show (2020)";
    scan_files(
        &store,
        "s1",
        "shows",
        root,
        vec![
            episode("Show (2020)/Season 01/Show.S01E01.mkv", old, 1, 1, FP_E1),
            episode("Show (2020)/Season 01/Show.S01E02.mkv", old, 1, 2, FP_E2),
        ],
        true,
    )
    .await;
    let asset1 = asset_at(&store, "shows", "Show (2020)/Season 01/Show.S01E01.mkv")
        .await
        .unwrap();
    let asset2 = asset_at(&store, "shows", "Show (2020)/Season 01/Show.S01E02.mkv")
        .await
        .unwrap();
    let series = series_of(&store, &asset1).await;
    let episode_work = work_of(&store, &asset1).await;
    for work in [&series, &episode_work] {
        query("INSERT INTO work_translations(work_id,locale,language,source,priority,title,updated_at) VALUES(?,'fr-FR','fr','manual',100,'Titre manuel',1)")
            .bind(work)
            .execute(store.pool())
            .await
            .unwrap();
    }
    let seasons_sql = "SELECT COUNT(*) FROM inventory_seasons WHERE series_id IN (SELECT id FROM inventory_works WHERE library_id=?)";
    let seasons_before = count(&store, seasons_sql, "shows").await;

    // The whole show folder is renamed (and so is every file).
    let new = "folder:Show Renamed (2020)";
    scan_files(
        &store,
        "s2",
        "shows",
        root,
        vec![
            episode("Show Renamed (2020)/S01/E01.mkv", new, 1, 1, FP_E1),
            episode("Show Renamed (2020)/S01/E02.mkv", new, 1, 2, FP_E2),
        ],
        true,
    )
    .await;

    assert_eq!(series_count(&store, "shows").await, 1, "no second series");
    assert_eq!(
        episodes_count(&store, "shows").await,
        2,
        "no second episodes"
    );
    assert_eq!(assets(&store, "shows").await, 2, "no second assets");
    assert_eq!(
        asset_at(&store, "shows", "Show Renamed (2020)/S01/E01.mkv")
            .await
            .as_deref(),
        Some(asset1.as_str())
    );
    assert_eq!(
        asset_at(&store, "shows", "Show Renamed (2020)/S01/E02.mkv")
            .await
            .as_deref(),
        Some(asset2.as_str())
    );
    assert_eq!(series_of(&store, &asset1).await, series);
    assert_eq!(work_of(&store, &asset1).await, episode_work);
    assert_eq!(count(&store, seasons_sql, "shows").await, seasons_before);
    let key: String = query_scalar("SELECT identity_key FROM inventory_works WHERE id=?")
        .bind(&series)
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert!(key.ends_with("\nfolder:Show Renamed (2020)"), "{key}");
    for work in [&series, &episode_work] {
        let manual: String =
            query_scalar("SELECT title FROM work_translations WHERE work_id=? AND source='manual'")
                .bind(work)
                .fetch_one(store.pool())
                .await
                .unwrap();
        assert_eq!(manual, "Titre manuel");
    }
    let alive = count(
        &store,
        "SELECT COUNT(*) FROM inventory_assets WHERE library_id=? AND available=1",
        "shows",
    )
    .await;
    assert_eq!(alive, 2);

    // Stable afterwards.
    scan_files(
        &store,
        "s3",
        "shows",
        root,
        vec![
            episode("Show Renamed (2020)/S01/E01.mkv", new, 1, 1, FP_E1),
            episode("Show Renamed (2020)/S01/E02.mkv", new, 1, 2, FP_E2),
        ],
        true,
    )
    .await;
    assert_eq!(
        (
            series_count(&store, "shows").await,
            episodes_count(&store, "shows").await,
            assets(&store, "shows").await
        ),
        (1, 2, 2)
    );
}

#[tokio::test]
async fn an_episode_file_renamed_inside_its_series_keeps_its_asset() {
    let temp = tempfile::tempdir().unwrap();
    let store = series_store(temp.path(), "shows").await;
    let key = "folder:Show (2020)";
    scan_files(
        &store,
        "s1",
        "shows",
        temp.path(),
        vec![episode(
            "Show (2020)/Show.S01E01.720p.mkv",
            key,
            1,
            1,
            FP_E1,
        )],
        true,
    )
    .await;
    let asset = asset_at(&store, "shows", "Show (2020)/Show.S01E01.720p.mkv")
        .await
        .unwrap();
    scan_files(
        &store,
        "s2",
        "shows",
        temp.path(),
        vec![episode(
            "Show (2020)/Show - S01E01 - Pilot.mkv",
            key,
            1,
            1,
            FP_E1,
        )],
        true,
    )
    .await;
    assert_eq!(assets(&store, "shows").await, 1);
    assert_eq!(
        asset_at(&store, "shows", "Show (2020)/Show - S01E01 - Pilot.mkv")
            .await
            .as_deref(),
        Some(asset.as_str())
    );
    assert_eq!(
        (
            series_count(&store, "shows").await,
            episodes_count(&store, "shows").await
        ),
        (1, 1)
    );
}

#[tokio::test]
async fn a_partial_series_move_or_a_renumbered_episode_relocates_nothing() {
    // Only one of two episodes moves to a new folder: the old series key is still in use.
    let temp = tempfile::tempdir().unwrap();
    let store = series_store(temp.path(), "shows").await;
    let old = "folder:Show (2020)";
    scan_files(
        &store,
        "s1",
        "shows",
        temp.path(),
        vec![
            episode("Show (2020)/S01E01.mkv", old, 1, 1, FP_E1),
            episode("Show (2020)/S01E02.mkv", old, 1, 2, FP_E2),
        ],
        true,
    )
    .await;
    let first = asset_at(&store, "shows", "Show (2020)/S01E01.mkv")
        .await
        .unwrap();
    scan_files(
        &store,
        "s2",
        "shows",
        temp.path(),
        vec![
            episode(
                "Elsewhere (2020)/S01E01.mkv",
                "folder:Elsewhere (2020)",
                1,
                1,
                FP_E1,
            ),
            episode("Show (2020)/S01E02.mkv", old, 1, 2, FP_E2),
        ],
        true,
    )
    .await;
    assert_eq!(
        series_count(&store, "shows").await,
        2,
        "two distinct series folders"
    );
    assert_ne!(
        asset_at(&store, "shows", "Elsewhere (2020)/S01E01.mkv")
            .await
            .unwrap(),
        first
    );

    // The same bytes under another episode number are another episode.
    let temp2 = tempfile::tempdir().unwrap();
    let store2 = series_store(temp2.path(), "shows").await;
    scan_files(
        &store2,
        "s1",
        "shows",
        temp2.path(),
        vec![episode("Show (2020)/S01E01.mkv", old, 1, 1, FP_E1)],
        true,
    )
    .await;
    let before = asset_at(&store2, "shows", "Show (2020)/S01E01.mkv")
        .await
        .unwrap();
    scan_files(
        &store2,
        "s2",
        "shows",
        temp2.path(),
        vec![episode("Show (2020)/S01E02.mkv", old, 1, 2, FP_E1)],
        true,
    )
    .await;
    assert_eq!(episodes_count(&store2, "shows").await, 2);
    assert_ne!(
        asset_at(&store2, "shows", "Show (2020)/S01E02.mkv")
            .await
            .unwrap(),
        before
    );
}
