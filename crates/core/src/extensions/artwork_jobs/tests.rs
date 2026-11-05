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
