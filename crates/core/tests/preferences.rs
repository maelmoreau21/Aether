use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
    response::Response,
};
use http_body_util::BodyExt;
use media_poc::{auth::AuthState, identity::IdentityStore, preferences};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tower::ServiceExt;

async fn request(app: &Router, method: &str, cookie: &str, csrf: &str, value: Value) -> Response {
    let mut req = Request::builder()
        .method(method)
        .uri("/api/v1/preferences")
        .header("origin", "http://127.0.0.1:3000")
        .header("content-type", "application/json");
    if !cookie.is_empty() {
        req = req.header("cookie", cookie);
    }
    if !csrf.is_empty() {
        req = req.header("x-csrf-token", csrf);
    }
    app.clone()
        .oneshot(req.body(Body::from(value.to_string())).unwrap())
        .await
        .unwrap()
}
async fn body(response: Response) -> Value {
    serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap()
}
#[tokio::test]
async fn preferences_are_account_owned_csrf_protected_bounded_and_persistent() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("prefs.sqlite");
    let store = IdentityStore::open(&path).await.unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let mut sessions = vec![];
    for (id, raw) in [("first", "a".repeat(64)), ("second", "b".repeat(64))] {
        media_poc::database::query("INSERT INTO users(id,role) VALUES(?,'user')")
            .bind(id)
            .execute(store.pool())
            .await
            .unwrap();
        let hash = format!("{:x}", Sha256::digest(raw.as_bytes()));
        media_poc::database::query("INSERT INTO sessions(token_hash,user_id,created_at,last_seen_at,expires_at) VALUES(?,?,?,?,?)")
            .bind(hash)
            .bind(id)
            .bind(now)
            .bind(now)
            .bind(now + 3600)
            .execute(store.pool())
            .await
            .unwrap();
        sessions.push((
            format!("media_session={raw}"),
            format!("{:x}", Sha256::digest(format!("csrf:{raw}").as_bytes())),
        ));
    }
    let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();
    let app = preferences::router(auth);
    let (cookie, csrf) = &sessions[0];
    assert_eq!(
        request(&app, "GET", "", "", json!(null)).await.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(&app, "PUT", cookie, "", json!({"theme":"dark"}))
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    for invalid in [
        json!({"userId":"second","theme":"dark"}),
        json!({"theme":"invalid"}),
        json!({"offsets":{"audio":31,"subtitle":0}}),
        json!({"subtitlePreference":{"enabled":true,"secret":"x"}}),
        json!({"audioLanguage":"x".repeat(8192)}),
    ] {
        assert!(
            request(&app, "PUT", cookie, csrf, invalid)
                .await
                .status()
                .is_client_error()
        );
    }
    let wanted = json!({"theme":"dark","locale":"en","audioLanguage":"fr","subtitles":"forced","audioTrack":{"mediaId":"media-1","trackId":"audio-2"},"offsets":{"audio":0.5,"subtitle":-1.0},"subtitlePreference":{"enabled":true,"language":"fr","forced":true}});
    assert_eq!(
        request(&app, "PUT", cookie, csrf, wanted.clone())
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        body(request(&app, "GET", cookie, "", json!(null)).await).await,
        wanted
    );
    assert_eq!(
        body(request(&app, "GET", &sessions[1].0, "", json!(null)).await).await,
        json!({})
    );
    assert_eq!(
        request(&app, "PUT", cookie, csrf, json!({"theme":"light"}))
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    let saved = body(request(&app, "GET", cookie, "", json!(null)).await).await;
    assert_eq!(saved["theme"], "light");
    assert_eq!(saved["audioLanguage"], "fr");
    let rotation = json!({"enabled":true,"enableMovies":true,"enableSeries":false,"rotationMode":"sequential","minHoursBetweenSwitches":24,"poolSize":3,"preferredLanguage":"fr-CA","fallbackLanguage":"en","includeUnknownLanguage":true,"allowAnyLanguageFallback":false});
    for invalid in [
        json!({"poolSize":0}),
        json!({"poolSize":25}),
        json!({"minHoursBetweenSwitches":721}),
        json!({"rotationMode":"unexpected"}),
        json!({"preferredLanguage":"../../"}),
    ] {
        let mut value = rotation.clone();
        for (key, field) in invalid.as_object().unwrap() {
            value[key] = field.clone();
        }
        assert_eq!(
            request(&app, "PUT", cookie, csrf, json!({"artworkRotation":value}))
                .await
                .status(),
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }
    assert_eq!(
        request(
            &app,
            "PUT",
            cookie,
            csrf,
            json!({"artworkRotation":rotation})
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    let saved = body(request(&app, "GET", cookie, "", json!(null)).await).await;
    assert_eq!(saved["artworkRotation"], rotation);
    assert_eq!(
        body(request(&app, "GET", &sessions[1].0, "", json!(null)).await).await,
        json!({})
    );
    drop(app);
    store.close().await;
    let reopened = IdentityStore::open(&path).await.unwrap();
    let app = preferences::router(
        AuthState::new(reopened.clone(), "http://127.0.0.1:3000".into())
            .await
            .unwrap(),
    );
    assert_eq!(
        body(request(&app, "GET", cookie, "", json!(null)).await).await,
        saved
    );
    reopened.close().await;
}
