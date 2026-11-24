use super::*;

#[tokio::test]
async fn durable_collection_resumes_an_expired_lease_and_revokes_cancelled_or_disabled_owners() {
    let temp = tempfile::tempdir().unwrap();
    let store = crate::identity::IdentityStore::open(&temp.path().join("jobs.sqlite"))
        .await
        .unwrap();
    crate::libraries::initialize_local(&store).await.unwrap();
    query("INSERT INTO users(id,role) VALUES('owner','administrator')")
        .execute(store.pool())
        .await
        .unwrap();
    query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('images','Images','films','local','unused',1)").execute(store.pool()).await.unwrap();
    query("INSERT INTO inventory_works(id,library_id,identity_key,title,external_ids_json) VALUES('work','images','work','Work','{\"tmdb\":\"1\"}')").execute(store.pool()).await.unwrap();
    let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();
    let parameters = serde_json::to_string(&Input {
        library_id: "images".into(),
        after: None,
        languages: vec!["fr".into(), "en".into()],
        include_neutral: true,
        max_images: 12,
        max_works: 1000,
        auto: None,
        since: None,
    })
    .unwrap();
    query("INSERT INTO artwork_collection_jobs(id,library_id,owner,parameters,status,lease_token,lease_until,created_at,updated_at) VALUES('job','images','owner',?,'running','expired',1,0,0)")
        .bind(parameters).execute(store.pool()).await.unwrap();
    tick(&auth).await.unwrap();
    let load = || {
        query_as::<_, Job>(sqlx::AssertSqlSafe(format!("{SELECT} WHERE id='job'")))
            .fetch_one(store.pool())
    };
    let job = load().await.unwrap();
    assert_eq!(job.status, "queued");
    assert_eq!(job.processed, 1);
    assert_eq!(job.cursor_work, "work");
    assert_ne!(job.lease_token, "expired");
    query("UPDATE artwork_collection_jobs SET next_run_at=0,status='running',lease_until=?,lease_token='live' WHERE id='job'").bind(now()+120).execute(store.pool()).await.unwrap();
    let job = load().await.unwrap();
    assert!(authorize_work(&auth, &job, "work").await.is_ok());
    query("UPDATE users SET disabled_at=? WHERE id='owner'")
        .bind(now())
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(
        authorize_work(&auth, &job, "work").await.unwrap_err().0,
        StatusCode::FORBIDDEN
    );
    query("UPDATE users SET disabled_at=NULL WHERE id='owner'")
        .execute(store.pool())
        .await
        .unwrap();
    query("UPDATE artwork_collection_jobs SET status='cancelled' WHERE id='job'")
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(
        authorize_work(&auth, &job, "work").await.unwrap_err().0,
        StatusCode::FORBIDDEN
    );
    query("UPDATE artwork_collection_jobs SET status='queued',next_run_at=0 WHERE id='job'")
        .execute(store.pool())
        .await
        .unwrap();
    // The native avatar gallery seeds its own assets: only new artwork counts.
    let seeded: i64 = query_scalar("SELECT COUNT(*) FROM assets")
        .fetch_one(store.pool())
        .await
        .unwrap();
    tick(&auth).await.unwrap();
    assert_eq!(load().await.unwrap().status, "completed");
    assert_eq!(
        query_scalar::<_, i64>("SELECT COUNT(*) FROM assets")
            .fetch_one(store.pool())
            .await
            .unwrap(),
        seeded
    );
}

fn synthetic_png(r: u8, g: u8, b: u8) -> Vec<u8> {
    let mut buf = std::io::Cursor::new(Vec::new());
    let img = image::RgbImage::from_pixel(16, 9, image::Rgb([r, g, b]));
    img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
    buf.into_inner()
}

#[tokio::test]
async fn episode_still_import_keeps_strictly_one_thumb_per_episode_and_respects_locks() {
    let temp = tempfile::tempdir().unwrap();
    let store = crate::identity::IdentityStore::open(&temp.path().join("ep-stills.sqlite"))
        .await
        .unwrap();
    crate::libraries::initialize_local(&store).await.unwrap();
    query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('shows','Shows','series','local','unused',1)")
        .execute(store.pool())
        .await
        .unwrap();
    query("INSERT INTO inventory_works(id,library_id,identity_key,title,external_ids_json,kind) VALUES('ep-1','shows','show/s01e01','Ep 1','{}','episode')")
        .execute(store.pool())
        .await
        .unwrap();
    query("INSERT INTO inventory_works(id,library_id,identity_key,title,external_ids_json,kind) VALUES('ep-2','shows','show/s01e02','Ep 2','{}','episode')")
        .execute(store.pool())
        .await
        .unwrap();

    // First import for ep-1 and ep-2 gives each episode its own single thumb.
    assert_eq!(
        adapters::store_episode_still_bytes(
            &store,
            "tmdb",
            "ep-1",
            "https://image.tmdb.org/t/p/w500/ep1.png",
            "image/png".into(),
            synthetic_png(220, 40, 40),
            false,
        )
        .await
        .unwrap(),
        adapters::EpisodeStillOutcome::Imported
    );
    assert_eq!(
        adapters::store_episode_still_bytes(
            &store,
            "tmdb",
            "ep-2",
            "https://image.tmdb.org/t/p/w500/ep2.png",
            "image/png".into(),
            synthetic_png(40, 200, 80),
            false,
        )
        .await
        .unwrap(),
        adapters::EpisodeStillOutcome::Imported
    );

    // Second non-replacing import skips cleanly — strictly 1 image per episode, never a pool.
    assert_eq!(
        adapters::store_episode_still_bytes(
            &store,
            "tmdb",
            "ep-1",
            "https://image.tmdb.org/t/p/w500/ep1-alt.png",
            "image/png".into(),
            synthetic_png(20, 20, 220),
            false,
        )
        .await
        .unwrap(),
        adapters::EpisodeStillOutcome::SkippedExisting
    );

    // Replacing with the same URL is a no-op; replacing with a new URL replaces in place (still 1 image).
    assert_eq!(
        adapters::store_episode_still_bytes(
            &store,
            "tmdb",
            "ep-1",
            "https://image.tmdb.org/t/p/w500/ep1.png",
            "image/png".into(),
            synthetic_png(220, 40, 40),
            true,
        )
        .await
        .unwrap(),
        adapters::EpisodeStillOutcome::SkippedExisting
    );
    assert_eq!(
        adapters::store_episode_still_bytes(
            &store,
            "tmdb",
            "ep-1",
            "https://image.tmdb.org/t/p/w500/ep1-v2.png",
            "image/png".into(),
            synthetic_png(40, 40, 220),
            true,
        )
        .await
        .unwrap(),
        adapters::EpisodeStillOutcome::Imported
    );

    // Locked episode artwork is never modified.
    query("UPDATE inventory_works SET manual_json='{\"artwork_locked\":true}' WHERE id='ep-2'")
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(
        adapters::store_episode_still_bytes(
            &store,
            "tmdb",
            "ep-2",
            "https://image.tmdb.org/t/p/w500/ep2-v2.png",
            "image/png".into(),
            synthetic_png(250, 250, 250),
            true,
        )
        .await
        .unwrap(),
        adapters::EpisodeStillOutcome::SkippedLocked
    );

    let count_ep1: i64 = query_scalar(
        "SELECT COUNT(*) FROM work_image_entries WHERE work_id='ep-1' AND kind='thumb'",
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    let count_ep2: i64 = query_scalar(
        "SELECT COUNT(*) FROM work_image_entries WHERE work_id='ep-2' AND kind='thumb'",
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(count_ep1, 1);
    assert_eq!(count_ep2, 1);
}

#[test]
fn pick_series_details_provider_prefers_tmdb_for_episode_stills() {
    let make_pkg = |id: &str| {
        (
            id.to_owned(),
            Package {
                manifest: Manifest {
                    id: id.to_owned(),
                    permissions: vec![
                        "metadata.read".into(),
                        "metadata.write".into(),
                        "images".into(),
                    ],
                    ..Default::default()
                },
                publisher_id: String::new(),
                wasm: String::new(),
                sha256: String::new(),
                signature: String::new(),
            },
        )
    };
    let extensions = vec![make_pkg("tvdb"), make_pkg("tmdb")];
    let options = crate::library_options::LibraryOptions::default();
    let mut ids = std::collections::BTreeMap::new();
    ids.insert("tvdb".into(), "81189".into());
    ids.insert("tmdb".into(), "1396".into());

    assert_eq!(
        pick_series_details_provider(&extensions, &options, &ids),
        Some(("tmdb".into(), "1396".into()))
    );

    ids.remove("tmdb");
    assert_eq!(
        pick_series_details_provider(&extensions, &options, &ids),
        Some(("tvdb".into(), "81189".into()))
    );
}
