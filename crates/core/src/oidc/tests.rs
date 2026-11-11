use super::{config::Config, http::routes, types::Oidc};
use crate::auth::{self, AuthState};
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::Response,
    routing::{get, post},
};
use axum::{body::Body, extract::Form, http::Request};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use http_body_util::BodyExt;
use jsonwebtoken::Algorithm;
use sha2::{Digest, Sha256};
use std::{collections::HashMap, sync::Mutex};
use std::{sync::Arc, time::Duration};
use tower::ServiceExt;
use url::Url;
use zeroize::Zeroizing;
const ORIGIN: &str = "http://127.0.0.1:3000";
const PASSWORD: &str = "synthetic local rescue password";
#[derive(Clone)]
struct Provider {
    issuer: String,
    claims: Arc<Mutex<serde_json::Value>>,
    key: Arc<std::sync::atomic::AtomicUsize>,
    exchanges: Arc<std::sync::atomic::AtomicUsize>,
}
struct Fixture {
    _temp: tempfile::TempDir,
    s: Oidc,
    provider: Provider,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn fixture() -> Fixture {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let issuer = format!(
        "http://{}/application/o/media/",
        listener.local_addr().unwrap()
    );
    let provider = Provider {
        issuer: issuer.clone(),
        claims: Arc::new(Mutex::new(serde_json::json!({}))),
        key: Arc::new(std::sync::atomic::AtomicUsize::new(1)),
        exchanges: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    };
    let app=Router::new().route("/application/o/media/.well-known/openid-configuration",get(|State(p): State<Provider>| async move { let base=Url::parse(&p.issuer).unwrap().origin().ascii_serialization(); Json(serde_json::json!({"issuer":p.issuer,"authorization_endpoint":format!("{base}/authorize"),"token_endpoint":format!("{base}/token"),"jwks_uri":format!("{base}/jwks"),"id_token_signing_alg_values_supported":["RS256"]})) }))
        .route("/jwks",get(|State(p): State<Provider>| async move { Json(serde_json::from_str::<serde_json::Value>(if p.key.load(std::sync::atomic::Ordering::SeqCst)==1 { include_str!("../../tests/fixtures/oidc/jwks1.json") } else { include_str!("../../tests/fixtures/oidc/jwks2.json") }).unwrap()) }))
        .route("/token",post(|State(p): State<Provider>, headers: HeaderMap, Form(form): Form<HashMap<String,String>>| async move {
            assert_eq!(headers[header::AUTHORIZATION],format!("Basic {}",base64::engine::general_purpose::STANDARD.encode("client:synthetic-secret")));
            assert_eq!(form["grant_type"],"authorization_code"); assert_eq!(form["redirect_uri"],format!("{ORIGIN}/api/v1/auth/oidc/callback")); assert_eq!(form["code_verifier"].len(),64);
            p.exchanges.fetch_add(1,std::sync::atomic::Ordering::SeqCst);
            let i=p.key.load(std::sync::atomic::Ordering::SeqCst);
            let key=jsonwebtoken::EncodingKey::from_rsa_pem(if i==1 { include_bytes!("../../tests/fixtures/oidc/key1.pem") } else { include_bytes!("../../tests/fixtures/oidc/key2.pem") }).unwrap();
            let mut h=jsonwebtoken::Header::new(Algorithm::RS256);h.kid=Some(format!("test-{i}"));
            let token=jsonwebtoken::encode(&h,&*p.claims.lock().unwrap(),&key).unwrap();
            Json(serde_json::json!({"id_token":token,"access_token":"never-exposed","refresh_token":"never-stored"}))
        })).with_state(provider.clone());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let temp = tempfile::tempdir().unwrap();
    // Test-only opt-in: the same provider/security cases run against PostgreSQL.
    // Each fixture receives an isolated schema in the disposable test database.
    let store = if let Ok(raw_url) = std::env::var("MEDIA_TEST_POSTGRES_URL") {
        sqlx::any::install_default_drivers();
        let admin = sqlx::AnyPool::connect(&raw_url).await.unwrap();
        let schema = format!("oidc_{}", uuid::Uuid::new_v4().simple());
        crate::database::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
        let mut url = url::Url::parse(&raw_url).unwrap();
        url.query_pairs_mut()
            .append_pair("options", &format!("-csearch_path={schema}"));
        crate::identity::IdentityStore::open_postgres(url.as_str())
            .await
            .unwrap()
    } else {
        crate::identity::IdentityStore::open(&temp.path().join("core.sqlite"))
            .await
            .unwrap()
    };
    store
        .bootstrap("rescue", Zeroizing::new(PASSWORD.into()))
        .await
        .unwrap();
    let auth = AuthState::new(store, ORIGIN.into()).await.unwrap();
    let c = Config {
        issuer,
        client_id: "client".into(),
        secret: Arc::new(Zeroizing::new("synthetic-secret".into())),
        redirect: format!("{ORIGIN}/api/v1/auth/oidc/callback"),
        scopes: "openid profile email".into(),
        groups: vec!["media-users".into()],
        admin_groups: vec![],
        groups_claim: "groups".into(),
        require_group: true,
    };
    let s = Oidc {
        auth,
        config: Some(c),
        http: reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap(),
        workers: Arc::new(tokio::sync::Semaphore::new(8)),
    };
    Fixture {
        _temp: temp,
        s,
        provider,
        task,
    }
}
#[tokio::test]
async fn public_status_reads_persisted_button_label_without_exposing_settings() {
    let f = fixture().await;
    crate::database::query("INSERT INTO server_settings(singleton,settings_json) VALUES(1,?)")
        .bind(serde_json::json!({"serverName":"Private server name","welcomeMessage":"Private welcome","ssoButtonLabel":"Connexion avec mon compte"}).to_string())
        .execute(&f.s.auth.store.pool).await.unwrap();
    let value = body(
        req(
            &f.s,
            "/api/v1/auth/oidc/status",
            "GET",
            "",
            "",
            serde_json::Value::Null,
        )
        .await,
    )
    .await;
    assert_eq!(value["buttonLabel"], "Connexion avec mon compte");
    assert!(value.get("serverName").is_none());
    assert!(value.get("welcomeMessage").is_none());
}

async fn body(r: Response) -> serde_json::Value {
    serde_json::from_slice(&r.into_body().collect().await.unwrap().to_bytes()).unwrap()
}
async fn req(
    s: &Oidc,
    path: &str,
    method: &str,
    cookie: &str,
    csrf: &str,
    data: serde_json::Value,
) -> Response {
    routes(s.clone())
        .merge(auth::router(s.auth.clone()))
        .oneshot(
            Request::builder()
                .uri(path)
                .method(method)
                .header(header::ORIGIN, ORIGIN)
                .header(header::COOKIE, cookie)
                .header("x-csrf-token", csrf)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(data.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
}
async fn begin(f: &Fixture) -> (String, String) {
    let r = req(
        &f.s,
        "/api/v1/auth/oidc/start",
        "POST",
        "",
        "",
        serde_json::Value::Null,
    )
    .await;
    assert_eq!(r.status(), StatusCode::OK);
    let cookie = r.headers()[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let data = body(r).await;
    let url = Url::parse(data["url"].as_str().unwrap()).unwrap();
    let pairs: HashMap<_, _> = url.query_pairs().into_owned().collect();
    assert_eq!(pairs["code_challenge_method"], "S256");
    assert_eq!(pairs["response_type"], "code");
    let nonce = pairs["nonce"].clone();
    let now = jsonwebtoken::get_current_timestamp();
    *f.provider.claims.lock().unwrap() = serde_json::json!({"iss":f.provider.issuer,"sub":"subject-1","aud":"client","iat":now,"exp":now+300,"nonce":nonce,"groups":["media-users"],"email":"rescue@example.test","role":"administrator"});
    let verifier: String =
        crate::database::query_scalar("SELECT verifier FROM oidc_flows WHERE state_hash=?")
            .bind(auth::hash(&pairs["state"]))
            .fetch_one(&f.s.auth.store.pool)
            .await
            .unwrap();
    assert_eq!(
        pairs["code_challenge"],
        URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
    );
    (cookie, pairs["state"].clone())
}
async fn finish(f: &Fixture, cookie: &str, state: &str) -> Response {
    req(
        &f.s,
        &format!("/api/v1/auth/oidc/callback?state={state}&code=synthetic-code"),
        "GET",
        cookie,
        "",
        serde_json::Value::Null,
    )
    .await
}
fn session_cookie(r: &Response) -> String {
    r.headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|h| h.to_str().ok())
        .find(|v| v.starts_with("aether_session="))
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned()
}
/// A request as a native application or the system browser sends it: no `Origin`.
async fn bare(
    s: &Oidc,
    path: &str,
    method: &str,
    cookie: &str,
    data: serde_json::Value,
) -> Response {
    routes(s.clone())
        .merge(auth::router(s.auth.clone()))
        .oneshot(
            Request::builder()
                .uri(path)
                .method(method)
                .header(header::COOKIE, cookie)
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::USER_AGENT, "Aether test phone")
                .body(Body::from(data.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
}
/// Starts the application flow in the "browser"; returns its flow cookie and state.
async fn app_begin(f: &Fixture, challenge: &str, redirect: &str) -> (String, String) {
    let r = bare(
        &f.s,
        &format!("/api/v1/auth/oidc/app/start?challenge={challenge}&redirect={redirect}"),
        "GET",
        "",
        serde_json::Value::Null,
    )
    .await;
    assert_eq!(r.status(), StatusCode::SEE_OTHER);
    let cookie = r.headers()[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let url = Url::parse(r.headers()[header::LOCATION].to_str().unwrap()).unwrap();
    let pairs: HashMap<_, _> = url.query_pairs().into_owned().collect();
    let now = jsonwebtoken::get_current_timestamp();
    *f.provider.claims.lock().unwrap() = serde_json::json!({"iss":f.provider.issuer,"sub":"subject-1","aud":"client","iat":now,"exp":now+300,"nonce":pairs["nonce"],"groups":["media-users"]});
    (cookie, pairs["state"].clone())
}
#[tokio::test]
async fn oidc_application_flow_hands_a_one_time_code_to_the_application_only() {
    let f = fixture().await;
    let verifier = auth::random_token();
    let challenge = auth::hash(&verifier);

    // Only the application's own scheme or a loopback port may receive the code.
    for redirect in [
        "https://evil.example/sso",
        "http://127.0.0.1:80/sso",
        "http://127.0.0.1:4000/other",
        "aether://sso/extra",
    ] {
        let r = bare(
            &f.s,
            &format!("/api/v1/auth/oidc/app/start?challenge={challenge}&redirect={redirect}"),
            "GET",
            "",
            serde_json::Value::Null,
        )
        .await;
        assert_eq!(r.status(), StatusCode::BAD_REQUEST, "{redirect}");
    }

    // Phone: the browser ends on a page that returns to the application, without a session.
    let (cookie, state) = app_begin(&f, &challenge, "aether://sso").await;
    let r = finish(&f, &cookie, &state).await;
    assert_eq!(r.status(), StatusCode::OK);
    assert!(
        !r.headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .any(|v| v.to_str().unwrap().starts_with("aether_session=")),
    );
    let page =
        String::from_utf8(r.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap();
    let code = page
        .split("aether://sso?code=")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
        .to_owned();
    assert_eq!(code.len(), 64);
    let sessions = || async {
        crate::database::query_scalar::<_, i64>("SELECT count(*) FROM sessions")
            .fetch_one(&f.s.auth.store.pool)
            .await
            .unwrap()
    };
    assert_eq!(sessions().await, 0);

    // A wrong verifier gets nothing and burns the code.
    let token = |code: &str, verifier: &str| serde_json::json!({"code":code,"verifier":verifier,"tokenType":"bearer"});
    let r = bare(
        &f.s,
        "/api/v1/auth/oidc/app/token",
        "POST",
        "",
        token(&code, &auth::random_token()),
    )
    .await;
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    let r = bare(
        &f.s,
        "/api/v1/auth/oidc/app/token",
        "POST",
        "",
        token(&code, &verifier),
    )
    .await;
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(sessions().await, 0);

    // Desktop: a loopback redirect, then the device token for the right verifier, once.
    let (cookie, state) = app_begin(&f, &challenge, "http://127.0.0.1:49152/sso").await;
    let r = finish(&f, &cookie, &state).await;
    assert_eq!(r.status(), StatusCode::SEE_OTHER);
    let location = r.headers()[header::LOCATION].to_str().unwrap().to_owned();
    let code = location
        .strip_prefix("http://127.0.0.1:49152/sso?code=")
        .unwrap()
        .to_owned();
    let r = bare(
        &f.s,
        "/api/v1/auth/oidc/app/token",
        "POST",
        "",
        token(&code, &verifier),
    )
    .await;
    assert_eq!(r.status(), StatusCode::OK);
    assert!(r.headers().get(header::SET_COOKIE).is_none());
    let granted = body(r).await;
    assert_eq!(granted["tokenType"], "bearer");
    assert_eq!(granted["accessToken"].as_str().unwrap().len(), 64);
    let label: String = crate::database::query_scalar("SELECT device_label FROM sessions")
        .fetch_one(&f.s.auth.store.pool)
        .await
        .unwrap();
    assert_eq!(label, "Aether test phone");
    let r = bare(
        &f.s,
        "/api/v1/auth/oidc/app/token",
        "POST",
        "",
        token(&code, &verifier),
    )
    .await;
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(sessions().await, 1);

    // A refusal by the provider comes back to the application as a reason, not a code.
    let (cookie, state) = app_begin(&f, &challenge, "http://127.0.0.1:49152/sso").await;
    let r = bare(
        &f.s,
        &format!("/api/v1/auth/oidc/callback?state={state}&error=access_denied"),
        "GET",
        &cookie,
        serde_json::Value::Null,
    )
    .await;
    assert_eq!(
        r.headers()[header::LOCATION],
        "http://127.0.0.1:49152/sso?error=oidc_invalid_response"
    );
}
#[tokio::test]
async fn oidc_sessions_rotation_roles_disabled_email_collision_and_local_regression() {
    let f = fixture().await;
    let (cookie, state) = begin(&f).await;
    let r = finish(&f, &cookie, &state).await;
    assert_eq!(r.status(), StatusCode::OK);
    let session = session_cookie(&r);
    assert!(
        r.headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .any(|v| v.to_str().unwrap().contains("SameSite=Strict"))
    );
    let me = body(
        req(
            &f.s,
            "/api/v1/auth/me",
            "GET",
            &session,
            "",
            serde_json::Value::Null,
        )
        .await,
    )
    .await;
    assert_eq!(me["user"]["role"], "user");
    let id = me["user"]["id"].as_str().unwrap();
    let count: i64 = crate::database::query_scalar("SELECT count(*) FROM users")
        .fetch_one(&f.s.auth.store.pool)
        .await
        .unwrap();
    assert_eq!(count, 2);
    let missing_avatars: i64 = crate::database::query_scalar("SELECT COUNT(*) FROM users u WHERE NOT EXISTS(SELECT 1 FROM user_avatars a WHERE a.user_id=u.id)")
        .fetch_one(&f.s.auth.store.pool).await.unwrap();
    assert_eq!(
        missing_avatars, 0,
        "bootstrap and SSO accounts must have an avatar"
    );

    assert_eq!(
        finish(&f, &cookie, &state).await.status(),
        StatusCode::UNAUTHORIZED
    );
    f.provider.key.store(2, std::sync::atomic::Ordering::SeqCst);
    let (cookie, state) = begin(&f).await;
    let r = finish(&f, &cookie, &state).await;
    assert_eq!(r.status(), StatusCode::OK);
    let count: i64 = crate::database::query_scalar("SELECT count(*) FROM users")
        .fetch_one(&f.s.auth.store.pool)
        .await
        .unwrap();
    assert_eq!(count, 2);
    crate::database::query("UPDATE users SET role='administrator' WHERE id=?")
        .bind(id)
        .execute(&f.s.auth.store.pool)
        .await
        .unwrap();
    let now = body(
        req(
            &f.s,
            "/api/v1/auth/me",
            "GET",
            &session,
            "",
            serde_json::Value::Null,
        )
        .await,
    )
    .await;
    assert_eq!(now["user"]["role"], "administrator");
    crate::database::query("UPDATE users SET role='user',disabled_at=unixepoch() WHERE id=?")
        .bind(id)
        .execute(&f.s.auth.store.pool)
        .await
        .unwrap();
    assert_eq!(
        req(
            &f.s,
            "/api/v1/auth/me",
            "GET",
            &session,
            "",
            serde_json::Value::Null
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    let (cookie, state) = begin(&f).await;
    assert_eq!(
        finish(&f, &cookie, &state).await.status(),
        StatusCode::FORBIDDEN
    );
    let r = req(
        &f.s,
        "/api/v1/auth/login",
        "POST",
        "",
        "",
        serde_json::json!({"login":"rescue","password":PASSWORD}),
    )
    .await;
    assert_eq!(r.status(), StatusCode::OK);
    let local = session_cookie(&r);
    let data = body(r).await;
    let csrf = data["csrfToken"].as_str().unwrap();
    let status = body(
        req(
            &f.s,
            "/api/v1/admin/oidc",
            "GET",
            &local,
            "",
            serde_json::Value::Null,
        )
        .await,
    )
    .await;
    assert_eq!(status["enabled"], true);
    assert!(!status.to_string().contains("synthetic-secret"));
    assert_eq!(
        req(
            &f.s,
            "/api/v1/auth/logout",
            "POST",
            &local,
            "bad",
            serde_json::Value::Null
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        req(
            &f.s,
            "/api/v1/auth/logout",
            "POST",
            &local,
            csrf,
            serde_json::Value::Null
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        req(
            &f.s,
            "/api/v1/auth/me",
            "GET",
            &local,
            "",
            serde_json::Value::Null
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
}
#[tokio::test]
async fn oidc_rejects_invalid_state_browser_nonce_signature_claims_and_groups() {
    let f = fixture().await;
    let (cookie, state) = begin(&f).await;
    assert_eq!(
        finish(&f, "", &state).await.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        finish(&f, &cookie, &"0".repeat(64)).await.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        f.provider
            .exchanges
            .load(std::sync::atomic::Ordering::SeqCst),
        0
    );
    for (field, value) in [
        ("nonce", serde_json::json!("bad")),
        ("iss", serde_json::json!("https://evil.test/")),
        ("aud", serde_json::json!("other")),
        ("exp", serde_json::json!(1)),
        ("iat", serde_json::json!(99999999999u64)),
        ("azp", serde_json::json!("other")),
        ("groups", serde_json::json!(["administrators"])),
    ] {
        let (cookie, state) = begin(&f).await;
        f.provider.claims.lock().unwrap()[field] = value;
        let expected = if field == "groups" {
            StatusCode::FORBIDDEN
        } else {
            StatusCode::UNAUTHORIZED
        };
        assert_eq!(
            finish(&f, &cookie, &state).await.status(),
            expected,
            "{field}"
        );
    }
    let (cookie, state) = begin(&f).await;
    let exchanges = f
        .provider
        .exchanges
        .load(std::sync::atomic::Ordering::SeqCst);
    let denied = req(
        &f.s,
        &format!("/api/v1/auth/oidc/callback?state={state}&error=access_denied"),
        "GET",
        &cookie,
        "",
        serde_json::Value::Null,
    )
    .await;
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        f.provider
            .exchanges
            .load(std::sync::atomic::Ordering::SeqCst),
        exchanges
    );
    assert_eq!(
        finish(&f, &cookie, &state).await.status(),
        StatusCode::UNAUTHORIZED
    );
    let (cookie, state) = begin(&f).await;
    crate::database::query("UPDATE oidc_flows SET expires_at=0")
        .execute(&f.s.auth.store.pool)
        .await
        .unwrap();
    assert_eq!(
        finish(&f, &cookie, &state).await.status(),
        StatusCode::UNAUTHORIZED
    );
    let (cookie, state) = begin(&f).await;
    f.task.abort();
    assert_eq!(
        finish(&f, &cookie, &state).await.status(),
        StatusCode::BAD_GATEWAY
    );
}
#[tokio::test]
async fn oidc_admin_group_does_not_promote_a_linked_local_user() {
    let mut f = fixture().await;
    f.s.config.as_mut().unwrap().admin_groups = vec!["media-admins".into()];
    crate::database::query("INSERT INTO users(id,role) VALUES('local-user','user')")
        .execute(&f.s.auth.store.pool)
        .await
        .unwrap();
    crate::database::query("INSERT INTO local_credentials(user_id,login,password_hash) SELECT 'local-user','local-user',password_hash FROM local_credentials WHERE login='rescue'")
        .execute(&f.s.auth.store.pool).await.unwrap();
    let login = req(
        &f.s,
        "/api/v1/auth/login",
        "POST",
        "",
        "",
        serde_json::json!({"login":"local-user","password":PASSWORD}),
    )
    .await;
    assert_eq!(login.status(), StatusCode::OK);
    let cookie = session_cookie(&login);
    let csrf = body(login).await["csrfToken"].as_str().unwrap().to_owned();
    let link = req(
        &f.s,
        "/api/v1/auth/oidc/link",
        "POST",
        &cookie,
        &csrf,
        serde_json::json!({"password":PASSWORD}),
    )
    .await;
    assert_eq!(link.status(), StatusCode::OK);
    let browser = link.headers()[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let url = Url::parse(body(link).await["url"].as_str().unwrap()).unwrap();
    let pairs: HashMap<_, _> = url.query_pairs().into_owned().collect();
    let now = jsonwebtoken::get_current_timestamp();
    *f.provider.claims.lock().unwrap() = serde_json::json!({"iss":f.provider.issuer,"sub":"local-subject","aud":"client","iat":now,"exp":now+300,"nonce":pairs["nonce"],"groups":["media-admins"]});
    let linked = finish(&f, &browser, &pairs["state"]).await;
    assert_eq!(linked.status(), StatusCode::OK);
    let role: String =
        crate::database::query_scalar("SELECT role FROM users WHERE id='local-user'")
            .fetch_one(&f.s.auth.store.pool)
            .await
            .unwrap();
    assert_eq!(role, "user");

    let (browser, state) = begin(&f).await;
    f.provider.claims.lock().unwrap()["sub"] = serde_json::json!("local-subject");
    f.provider.claims.lock().unwrap()["groups"] = serde_json::json!(["media-admins"]);
    assert_eq!(finish(&f, &browser, &state).await.status(), StatusCode::OK);
    let role: String =
        crate::database::query_scalar("SELECT role FROM users WHERE id='local-user'")
            .fetch_one(&f.s.auth.store.pool)
            .await
            .unwrap();
    assert_eq!(role, "user");
}

#[tokio::test]
async fn oidc_link_requires_local_proof_and_retains_rescue_account() {
    let f = fixture().await;
    let r = req(
        &f.s,
        "/api/v1/auth/login",
        "POST",
        "",
        "",
        serde_json::json!({"login":"rescue","password":PASSWORD}),
    )
    .await;
    let local = session_cookie(&r);
    let data = body(r).await;
    let csrf = data["csrfToken"].as_str().unwrap();
    let id = data["user"]["id"].as_str().unwrap();
    assert_eq!(
        req(
            &f.s,
            "/api/v1/auth/oidc/link",
            "POST",
            &local,
            csrf,
            serde_json::json!({"password":"wrong"})
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        req(
            &f.s,
            "/api/v1/auth/oidc/link",
            "POST",
            &local,
            "bad",
            serde_json::json!({"password":PASSWORD})
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    let (cookie, state) = begin(&f).await;
    // Real link endpoint supplies an original session hash after password reauthentication.
    let r = req(
        &f.s,
        "/api/v1/auth/oidc/link",
        "POST",
        &local,
        csrf,
        serde_json::json!({"password":PASSWORD}),
    )
    .await;
    assert_eq!(r.status(), StatusCode::OK);
    let link_cookie = r.headers()[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let data = body(r).await;
    let url = Url::parse(data["url"].as_str().unwrap()).unwrap();
    let pairs: HashMap<_, _> = url.query_pairs().into_owned().collect();
    f.provider.claims.lock().unwrap()["nonce"] = serde_json::json!(pairs["nonce"]);
    let r = finish(&f, &link_cookie, &pairs["state"]).await;
    assert_eq!(r.status(), StatusCode::OK);
    let linked = session_cookie(&r);
    let me = body(
        req(
            &f.s,
            "/api/v1/auth/me",
            "GET",
            &linked,
            "",
            serde_json::Value::Null,
        )
        .await,
    )
    .await;
    assert_eq!(me["user"]["id"], id);
    assert_eq!(me["user"]["role"], "administrator");
    assert_eq!(
        req(
            &f.s,
            "/api/v1/auth/me",
            "GET",
            &local,
            "",
            serde_json::Value::Null
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        finish(&f, &cookie, &state).await.status(),
        StatusCode::UNAUTHORIZED
    ); // nonce differs
    let count: i64 = crate::database::query_scalar("SELECT count(*) FROM users")
        .fetch_one(&f.s.auth.store.pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
    crate::database::query("UPDATE sessions SET last_seen_at=0")
        .execute(&f.s.auth.store.pool)
        .await
        .unwrap();
    assert_eq!(
        req(
            &f.s,
            "/api/v1/auth/me",
            "GET",
            &linked,
            "",
            serde_json::Value::Null
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        req(
            &f.s,
            "/api/v1/auth/login",
            "POST",
            "",
            "",
            serde_json::json!({"login":"rescue","password":PASSWORD})
        )
        .await
        .status(),
        StatusCode::OK
    );
}
#[tokio::test]
async fn oidc_signature_algorithm_audience_and_identity_collisions() {
    let mut f = fixture().await;
    let (_, _) = begin(&f).await;
    let d = f.s.discover().await.unwrap();
    let claims = f.provider.claims.lock().unwrap().clone();
    let nonce = claims["nonce"].as_str().unwrap();
    let mut h = jsonwebtoken::Header::new(Algorithm::RS256);
    h.kid = Some("test-1".into());
    let wrong = jsonwebtoken::EncodingKey::from_rsa_pem(include_bytes!(
        "../../tests/fixtures/oidc/key2.pem"
    ))
    .unwrap();
    let token = jsonwebtoken::encode(&h, &claims, &wrong).unwrap();
    assert!(f.s.claims(&d, &token, nonce).await.is_err());
    let hmac = jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &claims,
        &jsonwebtoken::EncodingKey::from_secret(b"not-an-RSA-key"),
    )
    .unwrap();
    assert!(f.s.claims(&d, &hmac, nonce).await.is_err());
    let key = jsonwebtoken::EncodingKey::from_rsa_pem(include_bytes!(
        "../../tests/fixtures/oidc/key1.pem"
    ))
    .unwrap();
    let mut multiple = claims.clone();
    multiple["aud"] = serde_json::json!(["client", "other"]);
    let token = jsonwebtoken::encode(&h, &multiple, &key).unwrap();
    assert!(f.s.claims(&d, &token, nonce).await.is_err());
    multiple["azp"] = serde_json::json!("client");
    let token = jsonwebtoken::encode(&h, &multiple, &key).unwrap();
    assert!(f.s.claims(&d, &token, nonce).await.is_ok());
    let (cookie, state) = begin(&f).await;
    assert_eq!(finish(&f, &cookie, &state).await.status(), StatusCode::OK);
    let (cookie, state) = begin(&f).await;
    f.provider.claims.lock().unwrap()["sub"] = serde_json::json!("different-subject-same-email");
    assert_eq!(finish(&f, &cookie, &state).await.status(), StatusCode::OK);
    let count: i64 = crate::database::query_scalar("SELECT count(*) FROM users")
        .fetch_one(&f.s.auth.store.pool)
        .await
        .unwrap();
    assert_eq!(count, 3);
    f.s.auth.secure = true;
    assert!(f.s.flow_cookie("test", 300).contains("; Secure"));
    assert!(f.s.auth.cookie("test", 300).contains("; Secure"));
    f.s.config = None;
    assert_eq!(
        body(
            req(
                &f.s,
                "/api/v1/auth/oidc/status",
                "GET",
                "",
                "",
                serde_json::Value::Null
            )
            .await
        )
        .await["enabled"],
        false
    );
    assert_eq!(
        req(
            &f.s,
            "/api/v1/auth/oidc/start",
            "POST",
            "",
            "",
            serde_json::Value::Null
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn oidc_generic_providers_keycloak_and_pocket_id_styles() {
    // Test 1: Keycloak / Dex style provider:
    // - Uses client_secret_post auth method
    // - Uses custom groups_claim ("roles")
    // - Has end_session_endpoint advertised
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let issuer = format!("http://{}/realms/media", listener.local_addr().unwrap());
    let claims = Arc::new(Mutex::new(serde_json::json!({})));
    let claims_for_app = claims.clone();
    let issuer_for_app = issuer.clone();

    let app = Router::new()
        .route(
            "/realms/media/.well-known/openid-configuration",
            get({
                let iss = issuer_for_app.clone();
                move || async move {
                    let base = Url::parse(&iss).unwrap().origin().ascii_serialization();
                    Json(serde_json::json!({
                        "issuer": iss,
                        "authorization_endpoint": format!("{base}/auth"),
                        "token_endpoint": format!("{base}/token"),
                        "jwks_uri": format!("{base}/jwks"),
                        "end_session_endpoint": format!("{base}/logout"),
                        "id_token_signing_alg_values_supported": ["RS256", "ES256"],
                        "token_endpoint_auth_methods_supported": ["client_secret_post"]
                    }))
                }
            }),
        )
        .route(
            "/jwks",
            get(|| async {
                Json(
                    serde_json::from_str::<serde_json::Value>(include_str!(
                        "../../tests/fixtures/oidc/jwks1.json"
                    ))
                    .unwrap(),
                )
            }),
        )
        .route(
            "/token",
            post({
                let claims_post = claims_for_app.clone();
                move |Form(form): Form<HashMap<String, String>>| async move {
                    assert_eq!(form["grant_type"], "authorization_code");
                    assert_eq!(form["client_id"], "keycloak-client");
                    assert_eq!(form["client_secret"], "keycloak-secret");
                    assert_eq!(
                        form["redirect_uri"],
                        format!("{ORIGIN}/api/v1/auth/oidc/callback")
                    );
                    let key = jsonwebtoken::EncodingKey::from_rsa_pem(include_bytes!(
                        "../../tests/fixtures/oidc/key1.pem"
                    ))
                    .unwrap();
                    let mut h = jsonwebtoken::Header::new(Algorithm::RS256);
                    h.kid = Some("test-1".into());
                    let token =
                        jsonwebtoken::encode(&h, &*claims_post.lock().unwrap(), &key).unwrap();
                    Json(serde_json::json!({"id_token": token}))
                }
            }),
        );

    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let temp = tempfile::tempdir().unwrap();
    let store = crate::identity::IdentityStore::open(&temp.path().join("core.sqlite"))
        .await
        .unwrap();
    store
        .bootstrap("rescue", Zeroizing::new(PASSWORD.into()))
        .await
        .unwrap();
    let auth = AuthState::new(store, ORIGIN.into()).await.unwrap();

    let config = Config {
        issuer: issuer.clone(),
        client_id: "keycloak-client".into(),
        secret: Arc::new(Zeroizing::new("keycloak-secret".into())),
        redirect: format!("{ORIGIN}/api/v1/auth/oidc/callback"),
        scopes: "openid profile email".into(),
        groups: vec!["media-access".into()],
        admin_groups: vec![],
        groups_claim: "roles".into(),
        require_group: true,
    };

    let s = Oidc {
        auth,
        config: Some(config),
        http: reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap(),
        workers: Arc::new(tokio::sync::Semaphore::new(8)),
    };

    let f = Fixture {
        _temp: temp,
        s,
        provider: Provider {
            issuer,
            claims,
            key: Arc::new(std::sync::atomic::AtomicUsize::new(1)),
            exchanges: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        },
        task,
    };

    // 1. Check logout URL discovery
    let logout_resp = req(
        &f.s,
        "/api/v1/auth/oidc/logout",
        "GET",
        "",
        "",
        serde_json::Value::Null,
    )
    .await;
    assert_eq!(logout_resp.status(), StatusCode::OK);
    let logout_data = body(logout_resp).await;
    let end_session = logout_data["endSessionUrl"].as_str().unwrap();
    assert!(end_session.contains("/logout"));
    assert!(end_session.contains("post_logout_redirect_uri="));

    // 2. Login with custom roles claim
    let (cookie, state) = begin(&f).await;
    let now = jsonwebtoken::get_current_timestamp();
    let nonce = f.provider.claims.lock().unwrap()["nonce"]
        .as_str()
        .unwrap()
        .to_string();
    *f.provider.claims.lock().unwrap() = serde_json::json!({
        "iss": f.provider.issuer,
        "sub": "keycloak-user-42",
        "aud": "keycloak-client",
        "iat": now,
        "exp": now + 300,
        "nonce": nonce,
        "roles": ["media-access"]
    });

    let r = finish(&f, &cookie, &state).await;
    assert_eq!(r.status(), StatusCode::OK);
    let session = session_cookie(&r);

    let me = body(
        req(
            &f.s,
            "/api/v1/auth/me",
            "GET",
            &session,
            "",
            serde_json::Value::Null,
        )
        .await,
    )
    .await;
    assert_eq!(me["user"]["role"], "user");

    // Test 2: Pocket ID style provider:
    // - require_group = false
    // - claims have no groups / roles claim at all
    let mut cfg_pocket = f.s.config.as_ref().unwrap().clone();
    cfg_pocket.require_group = false;
    cfg_pocket.groups.clear();
    let mut s_pocket = f.s.clone();
    s_pocket.config = Some(cfg_pocket);
    let f_pocket = Fixture {
        _temp: tempfile::tempdir().unwrap(),
        s: s_pocket,
        provider: f.provider.clone(),
        task: tokio::spawn(async {}),
    };

    let (cookie2, state2) = begin(&f_pocket).await;
    let nonce2 = f_pocket.provider.claims.lock().unwrap()["nonce"]
        .as_str()
        .unwrap()
        .to_string();
    *f_pocket.provider.claims.lock().unwrap() = serde_json::json!({
        "iss": f_pocket.provider.issuer,
        "sub": "pocket-id-user-99",
        "aud": "keycloak-client",
        "iat": now,
        "exp": now + 300,
        "nonce": nonce2
        // No groups or roles claim
    });

    let r2 = finish(&f_pocket, &cookie2, &state2).await;
    assert_eq!(r2.status(), StatusCode::OK);
    let session2 = session_cookie(&r2);

    let me2 = body(
        req(
            &f_pocket.s,
            "/api/v1/auth/me",
            "GET",
            &session2,
            "",
            serde_json::Value::Null,
        )
        .await,
    )
    .await;
    assert_eq!(me2["user"]["role"], "user");
}

#[tokio::test]
async fn oidc_admin_group_and_unicode_display_name() {
    let f = fixture().await;
    let mut cfg = f.s.config.as_ref().unwrap().clone();
    cfg.admin_groups = vec!["admingroup".into(), "Admins".into()];
    cfg.groups = vec!["media-users".into()];
    let mut s_custom = f.s.clone();
    s_custom.config = Some(cfg);
    let f_custom = Fixture {
        _temp: tempfile::tempdir().unwrap(),
        s: s_custom,
        provider: f.provider.clone(),
        task: tokio::spawn(async {}),
    };

    let (cookie, state) = begin(&f_custom).await;
    let nonce = f_custom.provider.claims.lock().unwrap()["nonce"]
        .as_str()
        .unwrap()
        .to_string();
    let now = jsonwebtoken::get_current_timestamp();
    *f_custom.provider.claims.lock().unwrap() = serde_json::json!({
        "iss": f_custom.provider.issuer,
        "sub": "sso-mael-id",
        "aud": "client",
        "iat": now,
        "exp": now + 300,
        "nonce": nonce,
        "groups": ["Admins"],
        "preferred_username": "mael",
        "name": "Maël Moreau"
    });

    let r = finish(&f_custom, &cookie, &state).await;
    assert_eq!(r.status(), StatusCode::OK);
    let session = session_cookie(&r);

    let me = body(
        req(
            &f_custom.s,
            "/api/v1/auth/me",
            "GET",
            &session,
            "",
            serde_json::Value::Null,
        )
        .await,
    )
    .await;

    assert_eq!(me["user"]["role"], "administrator");
    assert_eq!(me["user"]["login"], "mael");
    assert_eq!(me["user"]["displayName"], "Maël Moreau");

    // 2. User loses admin group in IdP (only has "media-users" now)
    let (cookie2, state2) = begin(&f_custom).await;
    let nonce2 = f_custom.provider.claims.lock().unwrap()["nonce"]
        .as_str()
        .unwrap()
        .to_string();
    *f_custom.provider.claims.lock().unwrap() = serde_json::json!({
        "iss": f_custom.provider.issuer,
        "sub": "sso-mael-id",
        "aud": "client",
        "iat": now + 10,
        "exp": now + 310,
        "nonce": nonce2,
        "groups": ["media-users"],
        "preferred_username": "mael",
        "name": "Maël Moreau"
    });

    let r2 = finish(&f_custom, &cookie2, &state2).await;
    assert_eq!(r2.status(), StatusCode::OK);
    let session2 = session_cookie(&r2);

    // 3. New session has downgraded role "user"
    let me2 = body(
        req(
            &f_custom.s,
            "/api/v1/auth/me",
            "GET",
            &session2,
            "",
            serde_json::Value::Null,
        )
        .await,
    )
    .await;
    assert_eq!(me2["user"]["role"], "user");

    // 4. Stale admin session was immediately revoked upon downgrade
    let old_session_res = req(
        &f_custom.s,
        "/api/v1/auth/me",
        "GET",
        &session,
        "",
        serde_json::Value::Null,
    )
    .await;
    assert_eq!(old_session_res.status(), StatusCode::UNAUTHORIZED);

    // 5. User loses all groups in IdP: login is denied and active sessions are revoked
    let (cookie3, state3) = begin(&f_custom).await;
    let nonce3 = f_custom.provider.claims.lock().unwrap()["nonce"]
        .as_str()
        .unwrap()
        .to_string();
    *f_custom.provider.claims.lock().unwrap() = serde_json::json!({
        "iss": f_custom.provider.issuer,
        "sub": "sso-mael-id",
        "aud": "client",
        "iat": now + 20,
        "exp": now + 320,
        "nonce": nonce3,
        "groups": ["unrelated-group"],
        "preferred_username": "mael"
    });

    let r3 = finish(&f_custom, &cookie3, &state3).await;
    assert_eq!(r3.status(), StatusCode::FORBIDDEN);

    // Session2 is now revoked
    let session2_after_removal = req(
        &f_custom.s,
        "/api/v1/auth/me",
        "GET",
        &session2,
        "",
        serde_json::Value::Null,
    )
    .await;
    assert_eq!(session2_after_removal.status(), StatusCode::UNAUTHORIZED);
}
