use super::*;

async fn fixture() -> (tempfile::TempDir, IdentityStore) {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("core.sqlite"))
        .await
        .unwrap();
    populate(&store).await;
    (temp, store)
}
async fn populate(store: &IdentityStore) {
    crate::libraries::initialize_local(store).await.unwrap();
    query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('images','Images','films','local','unused-read-only-root',1)").execute(store.pool()).await.unwrap();
    query("INSERT INTO inventory_works(id,library_id,identity_key,title,external_ids_json,editorial_json) VALUES('work','images','work','Work','{}','{\"original_language\":\"ja\"}')").execute(store.pool()).await.unwrap();
}
#[tokio::test]
#[ignore = "requires a disposable PostgreSQL 18 database and MEDIA_TEST_POSTGRES_URL"]
async fn postgres_webp_migration_contract() {
    let url = zeroize::Zeroizing::new(std::env::var("MEDIA_TEST_POSTGRES_URL").unwrap());
    let store = IdentityStore::open_postgres(&url).await.unwrap();
    populate(&store).await;
    contract(&store).await;
    let reopened = IdentityStore::open_postgres(&url).await.unwrap();
    assert_eq!(
        entries(&reopened, &["work".into()]).await.unwrap()[0].pinned,
        1
    );
}
fn source() -> Vec<u8> {
    let image = image::DynamicImage::ImageRgb8(image::RgbImage::from_fn(48, 72, |x, y| {
        image::Rgb([(x * 3) as u8, (y * 2) as u8, 80])
    }));
    let mut output = Cursor::new(Vec::new());
    image.write_to(&mut output, ImageFormat::Png).unwrap();
    output.into_inner()
}
async fn contract(store: &IdentityStore) {
    let source = source();
    let meta = store.assets.put_bytes(&source, "image/png").await.unwrap();
    query("INSERT INTO work_image_entries(id,work_id,kind,position,width,height,language,provider,source_url_hash) VALUES('old','work','poster',7,48,72,'ja','fixture','source-hash')").execute(store.pool()).await.unwrap();
    for width in [0, 320, 640, 1280] {
        query(
            "INSERT INTO work_image_data(image_id,width,mime,sha256) VALUES('old',?,'image/png',?)",
        )
        .bind(width)
        .bind(&meta.sha256)
        .execute(store.pool())
        .await
        .unwrap();
    }
    query("INSERT INTO artwork_annotations(work_id,image_id,kind,pinned) VALUES('work','old','poster',1)").execute(store.pool()).await.unwrap();
    query("INSERT INTO artwork_image_sources(work_id,source_url_hash,image_id) VALUES('work','source-hash','old')").execute(store.pool()).await.unwrap();
    assert!(migrate_one(store).await.unwrap());
    let images = entries(store, &["work".into()]).await.unwrap();
    assert_eq!(images.len(), 1);
    assert_eq!(images[0].id, "old");
    assert_eq!(images[0].pinned, 1);
    assert_eq!(images[0].language.as_deref(), Some("ja"));
    assert_eq!(images[0].provider.as_deref(), Some("fixture"));
    assert_eq!(
        original_language(store, "work").await.unwrap().as_deref(),
        Some("ja")
    );
    let refs: Vec<(String, String)> =
        query_as("SELECT mime,sha256 FROM work_image_data WHERE image_id='old'")
            .fetch_all(store.pool())
            .await
            .unwrap();
    assert_eq!(refs.len(), 4);
    for (mime, sha) in &refs {
        assert_eq!(mime, "image/webp");
        assert_ne!(sha, &meta.sha256);
        store.assets.verify(sha).await.unwrap();
    }
    assert_eq!(store.assets.read_all(&meta.sha256).await.unwrap().1, source);
    let before = refs.clone();
    assert!(!migrate_one(store).await.unwrap());
    assert_eq!(
        query_as::<_, (String, String)>(
            "SELECT mime,sha256 FROM work_image_data WHERE image_id='old'"
        )
        .fetch_all(store.pool())
        .await
        .unwrap(),
        before
    );
    let alias: String = query_scalar(
        "SELECT image_id FROM artwork_image_sources WHERE source_url_hash='source-hash'",
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(alias, "old");
}
#[tokio::test]
async fn migration_is_atomic_idempotent_and_preserves_identity_aliases_pins_and_old_bytes() {
    let (temp, store) = fixture().await;
    let media = temp.path().join("media");
    std::fs::create_dir(&media).unwrap();
    std::fs::write(media.join("poster.png"), source()).unwrap();
    let before = std::fs::read(media.join("poster.png")).unwrap();
    contract(&store).await;
    assert_eq!(std::fs::read(media.join("poster.png")).unwrap(), before);
    assert_eq!(std::fs::read_dir(media).unwrap().count(), 1);
}
#[tokio::test]
async fn malformed_legacy_image_keeps_all_references_and_records_retry() {
    let (_temp, store) = fixture().await;
    let meta = store
        .assets
        .put_bytes(b"invalid PNG", "image/png")
        .await
        .unwrap();
    query("INSERT INTO work_posters(work_id,revision,sha256) VALUES('work','same',?)")
        .bind(&meta.sha256)
        .execute(store.pool())
        .await
        .unwrap();
    assert!(migrate_one(&store).await.is_err());
    let sha: String = query_scalar("SELECT sha256 FROM work_posters WHERE work_id='work'")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(sha, meta.sha256);
    assert!(!migrate_one(&store).await.unwrap());
}
#[tokio::test]
async fn legacy_png_mislabeled_webp_migrates_shared_poster_library_and_previews_atomically() {
    let (_temp, store) = fixture().await;
    let meta = store
        .assets
        .put_bytes(&source(), "image/webp")
        .await
        .unwrap();
    query("INSERT INTO work_posters(work_id,sha256,revision) VALUES('work',?,'unchanged')")
        .bind(&meta.sha256)
        .execute(store.pool())
        .await
        .unwrap();
    query("INSERT INTO library_artwork(library_id,kind,sha256,revision) VALUES('images','poster',?,'same')").bind(&meta.sha256).execute(store.pool()).await.unwrap();
    assert!(migrate_one(&store).await.unwrap());
    let poster: String = query_scalar("SELECT sha256 FROM work_posters WHERE work_id='work'")
        .fetch_one(store.pool())
        .await
        .unwrap();
    let library: String =
        query_scalar("SELECT sha256 FROM library_artwork WHERE library_id='images'")
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(poster, library);
    assert_ne!(poster, meta.sha256);
    assert_eq!(
        query_scalar::<_, i64>("SELECT COUNT(*) FROM poster_previews WHERE work_id='work'")
            .fetch_one(store.pool())
            .await
            .unwrap(),
        3
    );
    assert_eq!(
        query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM library_artwork_previews WHERE library_id='images'"
        )
        .fetch_one(store.pool())
        .await
        .unwrap(),
        3
    );
    assert_eq!(
        image::guess_format(&store.assets.read_all(&poster).await.unwrap().1).unwrap(),
        ImageFormat::WebP
    );
    assert_eq!(
        store.assets.read_all(&meta.sha256).await.unwrap().1,
        source()
    );
    assert!(!migrate_one(&store).await.unwrap());
}
#[tokio::test]
async fn exact_content_reuses_webp_between_languages_and_imports_without_claiming_a_pin() {
    let (_temp, store) = fixture().await;
    let mut sha = None;
    for language in ["fr-CA", "en-US", "ja"] {
        let result = upload_internal(
            &store,
            "work".into(),
            "poster".into(),
            ImageQuery {
                append: true,
                language: Some(language.into()),
                provider: Some("fixture".into()),
                ..Default::default()
            },
            "image/png".into(),
            source().into(),
        )
        .await
        .unwrap();
        assert_eq!(result.0, StatusCode::CREATED);
        let images = entries(&store, &["work".into()]).await.unwrap();
        let image = images
            .iter()
            .find(|e| e.language.as_deref() == Some(&language.to_ascii_lowercase()))
            .unwrap();
        assert_eq!(image.pinned, 0);
        assert_eq!(image.original_release_verified, 0);
        if let Some(sha) = &sha {
            assert_eq!(image.content_hash.as_ref(), Some(sha));
        } else {
            sha = image.content_hash.clone();
        }
    }
    assert_eq!(
        query_scalar::<_, i64>("SELECT COUNT(*) FROM assets")
            .fetch_one(store.pool())
            .await
            .unwrap(),
        1
    );
    let digest = sha.unwrap();
    let bytes = store.assets.read_all(&digest).await.unwrap().1;
    let _ = upload_internal(
        &store,
        "work".into(),
        "poster".into(),
        ImageQuery {
            append: true,
            language: Some("de".into()),
            provider: Some("fixture".into()),
            ..Default::default()
        },
        "image/webp".into(),
        bytes.into(),
    )
    .await
    .unwrap();
    assert_eq!(
        query_scalar::<_, i64>("SELECT COUNT(*) FROM assets")
            .fetch_one(store.pool())
            .await
            .unwrap(),
        1
    );
}
