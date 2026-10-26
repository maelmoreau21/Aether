use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
    response::Response,
};
use http_body_util::BodyExt;
use media_poc::{
    admin_users,
    auth::{self, AuthState},
    avatars,
    database::query_scalar,
    identity::IdentityStore,
    server_settings,
};
use serde_json::{Value, json};
use tower::ServiceExt;
use zeroize::Zeroizing;
const ORIGIN: &str = "http://127.0.0.1:3000";
const PASSWORD: &str = "temporary account test password";
async fn request(
    app: &Router,
    path: &str,
    method: &str,
    cookie: &str,
    csrf: &str,
    mime: &str,
    body: Vec<u8>,
) -> Response {
    app.clone()
        .oneshot(
            Request::builder()
                .uri(path)
                .method(method)
                .header("origin", ORIGIN)
                .header("cookie", cookie)
                .header("x-csrf-token", csrf)
                .header("content-type", mime)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap()
}
async fn json_body(response: Response) -> Value {
    serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap()
}
async fn login(app: &Router, name: &str) -> (String, String) {
    let response = request(
        app,
        "/api/v1/auth/login",
        "POST",
        "",
        "",
        "application/json",
        json!({"login":name,"password":PASSWORD})
            .to_string()
            .into_bytes(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let cookie = response.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    (
        cookie,
        json_body(response).await["csrfToken"]
            .as_str()
            .unwrap()
            .into(),
    )
}
fn picture() -> Vec<u8> {
    let mut data = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(20, 10)
        .write_to(&mut data, image::ImageFormat::Png)
        .unwrap();
    data.into_inner()
}
#[tokio::test]
async fn settings_and_avatars_are_persistent_account_scoped_and_validated() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("test.sqlite");
    let store = IdentityStore::open(&path).await.unwrap();
    let admin = store
        .bootstrap("admin", Zeroizing::new(PASSWORD.into()))
        .await
        .unwrap();
    let auth = AuthState::new(store.clone(), ORIGIN.into()).await.unwrap();
    let app = auth::router(auth.clone())
        .merge(admin_users::router(auth.clone()))
        .merge(avatars::router(auth.clone()))
        .merge(server_settings::router(auth));
    for path in [
        "/api/v1/account/avatar",
        "/api/v1/admin/settings",
        "/api/v1/admin/network",
        "/api/v1/server",
    ] {
        assert_eq!(
            request(&app, path, "GET", "", "", "application/json", vec![])
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }
    let (cookie, csrf) = login(&app, "admin").await;
    let input =
        json!({"serverName":"Mon serveur","welcomeMessage":"Bienvenue <script>texte</script>","ssoButtonLabel":"Connexion avec mon compte"})
            .to_string()
            .into_bytes();
    assert_eq!(
        request(
            &app,
            "/api/v1/admin/settings",
            "PUT",
            &cookie,
            "",
            "application/json",
            input.clone()
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            &app,
            "/api/v1/admin/settings",
            "PUT",
            &cookie,
            &csrf,
            "application/json",
            input.clone()
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    let presentation = json_body(
        request(
            &app,
            "/api/v1/server",
            "GET",
            &cookie,
            "",
            "application/json",
            vec![],
        )
        .await,
    )
    .await;
    assert_eq!(presentation["serverName"], "Mon serveur");
    assert_eq!(presentation["ssoButtonLabel"], "Connexion avec mon compte");
    // Older clients updating the server name must preserve the custom SSO label.
    assert_eq!(
        request(
            &app,
            "/api/v1/admin/settings",
            "PUT",
            &cookie,
            &csrf,
            "application/json",
            json!({"serverName":"Mon serveur","welcomeMessage":"Bienvenue"})
                .to_string()
                .into_bytes()
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    let saved = json_body(
        request(
            &app,
            "/api/v1/admin/settings",
            "GET",
            &cookie,
            "",
            "application/json",
            vec![],
        )
        .await,
    )
    .await;
    assert_eq!(saved["ssoButtonLabel"], "Connexion avec mon compte");
    let network = json_body(
        request(
            &app,
            "/api/v1/admin/network",
            "GET",
            &cookie,
            "",
            "application/json",
            vec![],
        )
        .await,
    )
    .await;
    assert_eq!(
        request(
            &app,
            "/api/v1/admin/network",
            "PUT",
            &cookie,
            &csrf,
            "application/json",
            b"{}".to_vec()
        )
        .await
        .status(),
        StatusCode::METHOD_NOT_ALLOWED
    );
    assert_eq!(network["publicUrl"], ORIGIN);
    assert_eq!(network["secureCookies"], false);
    for input in [
        json!({"serverName":"","welcomeMessage":""}),
        json!({"serverName":"ok","welcomeMessage":"","ssoButtonLabel":" "}),
        json!({"serverName":"ok","welcomeMessage":"","ssoButtonLabel":"bad\nlabel"}),
        json!({"serverName":"ok","welcomeMessage":"","ssoButtonLabel":"x".repeat(81)}),
        json!({"serverName":"ok","welcomeMessage":"","publicUrl":"https://evil.example"}),
    ] {
        assert!(
            request(
                &app,
                "/api/v1/admin/settings",
                "PUT",
                &cookie,
                &csrf,
                "application/json",
                input.to_string().into_bytes()
            )
            .await
            .status()
            .is_client_error()
        );
    }
    assert_eq!(
        request(
            &app,
            "/api/v1/account/avatar",
            "PUT",
            &cookie,
            "",
            "image/png",
            picture()
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    for (mime, bytes) in [
        ("image/svg+xml", b"<svg></svg>".to_vec()),
        ("image/jpeg", picture()),
        ("image/png", b"broken".to_vec()),
    ] {
        assert!(
            request(
                &app,
                "/api/v1/account/avatar",
                "PUT",
                &cookie,
                &csrf,
                mime,
                bytes
            )
            .await
            .status()
            .is_client_error()
        );
    }
    assert_eq!(
        request(
            &app,
            "/api/v1/account/avatar",
            "PUT",
            &cookie,
            &csrf,
            "image/png",
            vec![0; 2 * 1024 * 1024 + 1]
        )
        .await
        .status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert_eq!(
        request(
            &app,
            "/api/v1/account/avatar",
            "PUT",
            &cookie,
            &csrf,
            "image/png",
            picture()
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    let response = request(
        &app,
        "/api/v1/account/avatar",
        "GET",
        &cookie,
        "",
        "image/png",
        vec![],
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let decoded = image::load_from_memory(&bytes).unwrap();
    assert_eq!((decoded.width(), decoded.height()), (256, 256));
    let head = request(
        &app,
        "/api/v1/account/avatar",
        "HEAD",
        &cookie,
        "",
        "image/png",
        vec![],
    )
    .await;
    assert_eq!(head.status(), StatusCode::OK);
    assert!(
        head.into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .is_empty()
    );
    assert_eq!(
        request(
            &app,
            "/api/v1/admin/users",
            "POST",
            &cookie,
            &csrf,
            "application/json",
            json!({"login":"viewer","password":PASSWORD,"role":"user"})
                .to_string()
                .into_bytes()
        )
        .await
        .status(),
        StatusCode::CREATED
    );
    let (other, other_csrf) = login(&app, "viewer").await;
    assert_eq!(
        request(
            &app,
            "/api/v1/admin/settings",
            "GET",
            &other,
            "",
            "application/json",
            vec![]
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            &app,
            "/api/v1/admin/network",
            "GET",
            &other,
            "",
            "application/json",
            vec![]
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            &app,
            "/api/v1/account/avatar",
            "GET",
            &other,
            "",
            "image/png",
            vec![]
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &app,
            "/api/v1/account/avatar",
            "DELETE",
            &other,
            &other_csrf,
            "image/png",
            vec![]
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request(
            &app,
            "/api/v1/account/avatar",
            "GET",
            &cookie,
            "",
            "image/png",
            vec![]
        )
        .await
        .status(),
        StatusCode::OK
    );
    store.close().await;
    let reopened = IdentityStore::open(&path).await.unwrap();
    assert_eq!(
        server_settings::load(&reopened).await.unwrap().server_name,
        "Mon serveur"
    );
    let sha: String = query_scalar("SELECT sha256 FROM user_avatars WHERE user_id=?")
        .bind(admin)
        .fetch_one(reopened.pool())
        .await
        .unwrap();
    let (_, persisted) = reopened.assets().read_all(&sha).await.unwrap();
    assert_eq!(persisted, bytes.to_vec());
    let auth = AuthState::new(reopened.clone(), ORIGIN.into())
        .await
        .unwrap();
    let app = auth::router(auth.clone()).merge(avatars::router(auth));
    let (cookie, csrf) = login(&app, "admin").await;
    assert_eq!(
        request(
            &app,
            "/api/v1/account/avatar",
            "DELETE",
            &cookie,
            &csrf,
            "image/png",
            vec![]
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request(
            &app,
            "/api/v1/account/avatar",
            "GET",
            &cookie,
            "",
            "image/png",
            vec![]
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    reopened.close().await;
}
