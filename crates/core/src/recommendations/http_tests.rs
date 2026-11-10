//! Account/permission/history contracts through the real recommendations router.
use super::*;
use axum::{body::Body, http::Request};
use http_body_util::BodyExt;
use tower::ServiceExt;

const ORIGIN: &str = "http://127.0.0.1:3000";

async fn call(
    app: &Router,
    method: &str,
    path: &str,
    cookie: &str,
    csrf: &str,
    body: Value,
) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header(header::COOKIE, cookie)
                .header(header::ORIGIN, ORIGIN)
                .header("x-csrf-token", csrf)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    assert_eq!(
        response.headers()[header::CACHE_CONTROL],
        "private, no-store"
    );
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn session(auth: &AuthState, user: &str, token: &str) -> (String, String) {
    query("INSERT INTO sessions(token_hash,user_id,created_at,last_seen_at,expires_at) VALUES(?,?,?,?,?)")
        .bind(crate::auth::hash(token)).bind(user).bind(media_contracts::protocol::unix_now()).bind(media_contracts::protocol::unix_now()).bind(media_contracts::protocol::unix_now()+3600)
        .execute(auth.store.pool()).await.unwrap();
    let cookie = format!("media_session={token}");
    let mut headers = HeaderMap::new();
    headers.insert(header::COOKIE, cookie.parse().unwrap());
    let csrf = auth.authenticate(&headers).await.unwrap().csrf_token;
    (cookie, csrf)
}

async fn fixture(store: &crate::identity::IdentityStore) {
    for id in ["reader", "other"] {
        query("INSERT INTO users(id,role) VALUES(?,'user')")
            .bind(id)
            .execute(store.pool())
            .await
            .unwrap();
    }
    for id in ["visible", "seeds", "hidden"] {
        query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES(?,?,'films','local',?,1)")
            .bind(id).bind(id).bind(format!("/read-only/{id}")).execute(store.pool()).await.unwrap();
    }
    for user in ["reader", "other"] {
        for library in ["visible", "seeds"] {
            query("INSERT INTO library_permissions(library_id,user_id) VALUES(?,?)")
                .bind(library)
                .bind(user)
                .execute(store.pool())
                .await
                .unwrap();
        }
    }
    for (id, lib, title, genre, age) in [
        ("seed", "seeds", "Visible seed", "Drama", 0),
        ("near", "visible", "Related candidate", "Drama", 0),
        ("comedy", "visible", "Unrelated candidate", "Comedy", 0),
        ("disliked", "visible", "Disliked candidate", "Drama", 0),
        ("adult", "visible", "Adult candidate", "Drama", 21),
        ("hidden-seed", "hidden", "PRIVATE seed title", "Drama", 0),
    ] {
        let editorial=json!({"genres":[genre],"directors":[if genre=="Drama" {"Shared director"} else {"Other director"}]}).to_string();
        query("INSERT INTO inventory_works(id,library_id,identity_key,title,title_sort,kind,external_ids_json,editorial_json,has_available_asset,minimum_age,added_at) VALUES(?,?,?,?,?,'film','{}',?,1,?,?)")
            .bind(id).bind(lib).bind(id).bind(title).bind(title.to_lowercase()).bind(editorial).bind(age).bind(media_contracts::protocol::unix_now()).execute(store.pool()).await.unwrap();
    }
    query("INSERT INTO parental_policies(user_id,maximum_age,allow_unrated) VALUES('reader',12,1)")
        .execute(store.pool())
        .await
        .unwrap();
}

#[tokio::test]
async fn account_http_recommendations_default_on_and_respect_private_persistent_preferences() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("recommendations.sqlite");
    let store = crate::identity::IdentityStore::open(&path).await.unwrap();
    fixture(&store).await;
    let auth = AuthState::new(store.clone(), ORIGIN.into()).await.unwrap();
    let (cookie, csrf) = session(&auth, "reader", &"a".repeat(64)).await;
    let (other_cookie, other_csrf) = session(&auth, "other", &"b".repeat(64)).await;
    let app = router(auth.clone());
    let endpoint = "/api/v1/account/recommendations";
    let prefs = "/api/v1/account/recommendations/preferences";
    assert_eq!(
        call(&app, "GET", endpoint, "", "", Value::Null).await.0,
        StatusCode::UNAUTHORIZED
    );
    let (_, default) = call(&app, "GET", endpoint, &cookie, "", Value::Null).await;
    assert_eq!(default["enabled"], true);
    assert_eq!(default["mode"], "discovery");
    assert!(!default["items"].as_array().unwrap().is_empty());
    let (_, defaults) = call(&app, "GET", prefs, &cookie, "", Value::Null).await;
    assert_eq!(
        defaults,
        json!({"enabled":true,"excludeWatched":true,"historyDays":180})
    );
    let enabled = json!({"enabled":true,"excludeWatched":true,"historyDays":30});
    assert_eq!(
        call(&app, "PUT", prefs, &cookie, "", enabled.clone())
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    for days in [0, 3651] {
        assert_eq!(
            call(
                &app,
                "PUT",
                prefs,
                &cookie,
                &csrf,
                json!({"enabled":true,"excludeWatched":true,"historyDays":days})
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(
        call(
            &app,
            "PUT",
            prefs,
            &cookie,
            &csrf,
            json!({"enabled":true,"excludeWatched":true,"historyDays":30,"userId":"other"})
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        call(&app, "PUT", prefs, &cookie, &csrf, enabled.clone())
            .await
            .0,
        StatusCode::OK
    );
    let (_, discovery) = call(&app, "GET", endpoint, &cookie, "", Value::Null).await;
    assert_eq!(discovery["mode"], "discovery");
    assert!(!discovery["items"].as_array().unwrap().is_empty());
    assert!(!discovery.to_string().contains("PRIVATE seed title"));
    assert!(!discovery.to_string().contains("Adult candidate"));
    query("INSERT INTO work_feedback(user_id,work_id,favorite,rating) VALUES('reader','hidden-seed',1,5.0)").execute(store.pool()).await.unwrap();
    let (_, hidden_only) = call(&app, "GET", endpoint, &cookie, "", Value::Null).await;
    assert_eq!(hidden_only["mode"], "discovery");
    assert!(!hidden_only.to_string().contains("PRIVATE seed title"));
    query("INSERT INTO work_feedback(user_id,work_id,favorite,rating) VALUES('reader','seed',1,4.5),('reader','disliked',0,1.0)").execute(store.pool()).await.unwrap();
    query("INSERT INTO playback_progress(user_id,work_id,position_seconds,duration_seconds,watched,revision,updated_at) VALUES('reader','seed',100.0,100.0,1,1,?)")
        .bind(media_contracts::protocol::unix_now()).execute(store.pool()).await.unwrap();
    let (_, ranked) = call(&app, "GET", endpoint, &cookie, "", Value::Null).await;
    assert_eq!(ranked["mode"], "personalized");
    let items = ranked["items"].as_array().unwrap();
    assert_eq!(items[0]["id"], "near");
    assert!(
        items[0]["recommendationReason"]
            .as_str()
            .unwrap()
            .contains("Visible seed")
    );
    assert!(
        !items
            .iter()
            .any(|i| i["id"] == "seed" || i["id"] == "disliked")
    );
    assert_eq!(
        call(&app, "PUT", prefs, &other_cookie, &other_csrf, enabled)
            .await
            .0,
        StatusCode::OK
    );
    let (_, other) = call(&app, "GET", endpoint, &other_cookie, "", Value::Null).await;
    assert_eq!(other["mode"], "discovery");
    assert!(
        other["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["id"] == "seed")
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            prefs,
            &other_cookie,
            &other_csrf,
            json!({"enabled":false,"excludeWatched":true,"historyDays":30})
        )
        .await
        .0,
        StatusCode::OK
    );
    // Catalogue edits must invalidate affinity/explanations, not only the shelf's cards.
    query("UPDATE inventory_works SET editorial_json=? WHERE id='near'")
        .bind(json!({"genres":["Comedy"],"directors":["Other director"]}).to_string())
        .execute(store.pool())
        .await
        .unwrap();
    let (_, edited) = call(&app, "GET", endpoint, &cookie, "", Value::Null).await;
    assert_eq!(
        edited["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["id"] == "near")
            .unwrap()["recommendationReason"],
        "À découvrir dans votre catalogue"
    );
    // A seed that became invisible must not leak through the structured explanation either.
    assert!(
        edited["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["id"] == "near")
            .unwrap()["recommendationBecause"]
            .is_null()
    );
    query("DELETE FROM work_feedback WHERE user_id='reader' AND work_id='seed'")
        .execute(store.pool())
        .await
        .unwrap();
    query("UPDATE playback_progress SET updated_at=? WHERE user_id='reader'")
        .bind(media_contracts::protocol::unix_now() - 10 * 86400)
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(
        call(
            &app,
            "PUT",
            prefs,
            &cookie,
            &csrf,
            json!({"enabled":true,"excludeWatched":true,"historyDays":1})
        )
        .await
        .0,
        StatusCode::OK
    );
    let (_, expired_history) = call(&app, "GET", endpoint, &cookie, "", Value::Null).await;
    assert_eq!(expired_history["mode"], "discovery");
    assert_eq!(
        call(
            &app,
            "PUT",
            prefs,
            &cookie,
            &csrf,
            json!({"enabled":true,"excludeWatched":false,"historyDays":30})
        )
        .await
        .0,
        StatusCode::OK
    );
    let (_, history) = call(&app, "GET", endpoint, &cookie, "", Value::Null).await;
    assert_eq!(history["mode"], "personalized");
    assert!(
        history["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["id"] == "seed")
    );
    query("DELETE FROM library_permissions WHERE library_id='seeds' AND user_id='reader'")
        .execute(store.pool())
        .await
        .unwrap();
    let (_, revoked) = call(&app, "GET", endpoint, &cookie, "", Value::Null).await;
    assert_eq!(revoked["mode"], "discovery");
    assert!(!revoked.to_string().contains("Visible seed"));
    drop(app);
    drop(auth);
    store.close().await;
    let reopened = crate::identity::IdentityStore::open(&path).await.unwrap();
    let auth = AuthState::new(reopened.clone(), ORIGIN.into())
        .await
        .unwrap();
    let app = router(auth);
    // An existing opt-out survives reopening; the new default applies only to absent preferences.
    let (_, other_stored) = call(&app, "GET", prefs, &other_cookie, "", Value::Null).await;
    assert_eq!(other_stored["enabled"], false);
    let (_, other_disabled) = call(&app, "GET", endpoint, &other_cookie, "", Value::Null).await;
    assert_eq!(other_disabled["mode"], "disabled");
    let (_, stored) = call(&app, "GET", prefs, &cookie, "", Value::Null).await;
    assert_eq!(
        stored,
        json!({"enabled":true,"excludeWatched":false,"historyDays":30})
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            prefs,
            &cookie,
            &csrf,
            json!({"enabled":false,"excludeWatched":false,"historyDays":30})
        )
        .await
        .0,
        StatusCode::OK
    );
    let (_, disabled) = call(&app, "GET", endpoint, &cookie, "", Value::Null).await;
    assert_eq!(disabled["mode"], "disabled");
    assert!(disabled["items"].as_array().unwrap().is_empty());
}
