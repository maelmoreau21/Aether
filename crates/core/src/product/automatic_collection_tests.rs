use super::*;
#[tokio::test]
async fn collections_follow_opt_in_membership_and_live_permissions() {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("collections.sqlite"))
        .await
        .unwrap();
    query("INSERT INTO users(id,role) VALUES('reader','user')")
        .execute(store.pool())
        .await
        .unwrap();
    let token = "c".repeat(64);
    query("INSERT INTO sessions(token_hash,user_id,created_at,last_seen_at,expires_at) VALUES(?,'reader',unixepoch(),unixepoch(),unixepoch()+3600)").bind(crate::auth::hash(&token)).execute(store.pool()).await.unwrap();
    query("INSERT INTO libraries(id,name,kind,node_id,root,enabled,options_json) VALUES('films','Films','films','local','/synthetic-read-only',1,'{\"automaticCollections\":true}')").execute(store.pool()).await.unwrap();
    query("INSERT INTO library_permissions(library_id,user_id) VALUES('films','reader')")
        .execute(store.pool())
        .await
        .unwrap();
    for id in ["one", "two"] {
        query("INSERT INTO inventory_works(id,library_id,identity_key,title,kind,external_ids_json,editorial_json,has_available_asset) VALUES(?,'films',?,?,'film','{}',?,1)")
            .bind(id).bind(id).bind(id).bind(json!({"collection":{"id":"tmdb:42","name":"Legal synthetic saga"}}).to_string()).execute(store.pool()).await.unwrap();
    }
    let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();
    let mut headers = HeaderMap::new();
    headers.insert(
        header::COOKIE,
        format!("aether_session={token}").parse().unwrap(),
    );
    let actor = auth.authenticate(&headers).await.unwrap();
    let collections = automatic_collections(&auth, &actor).await.unwrap();
    assert_eq!(collections.len(), 1);
    let collection_id = collections.keys().next().unwrap().clone();
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    let request = |path: &str| {
        axum::http::Request::builder()
            .uri(path)
            .header(header::COOKIE, format!("aether_session={token}"))
            .body(axum::body::Body::empty())
            .unwrap()
    };
    let response = router(auth.clone())
        .oneshot(request("/api/v1/lists"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let payload: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(payload["lists"][0]["automatic"], true);
    assert_eq!(payload["lists"][0]["owned"], false);
    let response = router(auth.clone())
        .oneshot(request(&format!(
            "/api/v1/lists/{collection_id}?compact=true"
        )))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let detail: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(detail["entries"].as_array().unwrap().len(), 2);

    assert_eq!(collections.values().next().unwrap().1.len(), 2);
    query("UPDATE inventory_works SET minimum_age=18 WHERE id='two'")
        .execute(store.pool())
        .await
        .unwrap();
    query("INSERT INTO parental_policies(user_id,maximum_age,allow_unrated) VALUES('reader',12,1)")
        .execute(store.pool())
        .await
        .unwrap();
    assert!(
        automatic_collections(&auth, &actor)
            .await
            .unwrap()
            .is_empty(),
        "a hidden film cannot expose a saga"
    );
    query("DELETE FROM parental_policies")
        .execute(store.pool())
        .await
        .unwrap();
    query("DELETE FROM library_permissions")
        .execute(store.pool())
        .await
        .unwrap();
    let response = router(auth.clone())
        .oneshot(request(&format!("/api/v1/lists/{collection_id}")))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    assert!(
        automatic_collections(&auth, &actor)
            .await
            .unwrap()
            .is_empty()
    );
    query("INSERT INTO library_permissions(library_id,user_id) VALUES('films','reader')")
        .execute(store.pool())
        .await
        .unwrap();
    query("UPDATE libraries SET options_json='{}'")
        .execute(store.pool())
        .await
        .unwrap();
    assert!(
        automatic_collections(&auth, &actor)
            .await
            .unwrap()
            .is_empty()
    );
}
#[tokio::test]
async fn favourites_become_one_list_per_type_and_can_be_switched_off() {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("favourites.sqlite"))
        .await
        .unwrap();
    query("INSERT INTO users(id,role) VALUES('reader','user')")
        .execute(store.pool())
        .await
        .unwrap();
    let token = "d".repeat(64);
    query("INSERT INTO sessions(token_hash,user_id,created_at,last_seen_at,expires_at) VALUES(?,'reader',unixepoch(),unixepoch(),unixepoch()+3600)").bind(crate::auth::hash(&token)).execute(store.pool()).await.unwrap();
    for (library, kind) in [("films", "films"), ("shows", "series")] {
        query("INSERT INTO libraries(id,name,kind,node_id,root,enabled,options_json) VALUES(?,?,?,'local',?,1,'{}')")
            .bind(library).bind(library).bind(kind).bind(format!("/synthetic-{library}")).execute(store.pool()).await.unwrap();
        query("INSERT INTO library_permissions(library_id,user_id) VALUES(?,'reader')")
            .bind(library)
            .execute(store.pool())
            .await
            .unwrap();
    }
    for (id, library, kind, favorite) in [
        ("loved-film", "films", "film", 1),
        ("plain-film", "films", "film", 0),
        ("loved-show", "shows", "series", 1),
    ] {
        query("INSERT INTO inventory_works(id,library_id,identity_key,title,kind,external_ids_json,editorial_json,has_available_asset) VALUES(?,?,?,?,?,'{}','{}',1)")
            .bind(id).bind(library).bind(id).bind(id).bind(kind).execute(store.pool()).await.unwrap();
        query("INSERT INTO work_feedback(user_id,work_id,favorite) VALUES('reader',?,?)")
            .bind(id)
            .bind(favorite)
            .execute(store.pool())
            .await
            .unwrap();
    }
    let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();
    let mut headers = HeaderMap::new();
    headers.insert(
        header::COOKIE,
        format!("aether_session={token}").parse().unwrap(),
    );
    let actor = auth.authenticate(&headers).await.unwrap();

    // On by default: one list per type, hearts only.
    let lists = automatic_favorites(&auth, &actor).await.unwrap();
    assert_eq!(lists.len(), 2);
    assert_eq!(lists[FAVORITE_FILMS].1.len(), 1);
    assert_eq!(lists[FAVORITE_FILMS].1[0]["id"], "loved-film");
    assert_eq!(lists[FAVORITE_SERIES].1[0]["id"], "loved-show");

    // Served through the same routes as the other lists, with a translatable key.
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    let request = |path: &str| {
        axum::http::Request::builder()
            .uri(path)
            .header(header::COOKIE, format!("aether_session={token}"))
            .body(axum::body::Body::empty())
            .unwrap()
    };
    let response = router(auth.clone())
        .oneshot(request("/api/v1/lists"))
        .await
        .unwrap();
    let payload: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let favourite = payload["lists"]
        .as_array()
        .unwrap()
        .iter()
        .find(|list| list["id"] == FAVORITE_FILMS)
        .unwrap();
    assert_eq!(favourite["systemKey"], "favorites-film");
    assert_eq!(favourite["owned"], false);
    let response = router(auth.clone())
        .oneshot(request(&format!(
            "/api/v1/lists/{FAVORITE_FILMS}?compact=true"
        )))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // A library the account lost never leaks through its hearts.
    query("DELETE FROM library_permissions WHERE library_id='shows'")
        .execute(store.pool())
        .await
        .unwrap();
    let lists = automatic_favorites(&auth, &actor).await.unwrap();
    assert!(lists.contains_key(FAVORITE_FILMS) && !lists.contains_key(FAVORITE_SERIES));

    // Switched off in the settings: nothing is derived.
    query("INSERT INTO user_preferences(user_id,preferences_json) VALUES('reader','{\"autoPlaylists\":false}')")
        .execute(store.pool())
        .await
        .unwrap();
    assert!(automatic_favorites(&auth, &actor).await.unwrap().is_empty());
}
