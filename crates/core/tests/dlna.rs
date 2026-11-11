//! DLNA ContentDirectory over a real store: what a `catalog:read` key may read
//! is exactly what the network sees, and a revoked key shows nothing.
use aether_core::{
    admin_users, api_keys,
    auth::{self, AuthState},
    database::query,
    dlna::{DlnaConfig, router},
    identity::IdentityStore,
};
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
    response::Response,
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use zeroize::Zeroizing;

const ORIGIN: &str = "http://127.0.0.1:3000";
const PASSWORD: &str = "integration account strong secret";

async fn send(app: &Router, request: Request<Body>) -> Response {
    app.clone().oneshot(request).await.unwrap()
}
async fn text(response: Response) -> String {
    String::from_utf8(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap()
}
async fn json_req(
    app: &Router,
    path: &str,
    method: &str,
    cookie: &str,
    csrf: &str,
    body: Value,
) -> Response {
    let mut request = Request::builder()
        .uri(path)
        .method(method)
        .header("content-type", "application/json")
        .header("origin", ORIGIN);
    if !cookie.is_empty() {
        request = request
            .header("cookie", cookie)
            .header("x-csrf-token", csrf);
    }
    send(app, request.body(Body::from(body.to_string())).unwrap()).await
}
fn browse(secret: &str, object: &str, flag: &str) -> Request<Body> {
    let body = format!(
        r#"<?xml version="1.0"?><s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/"><s:Body><u:Browse xmlns:u="urn:schemas-upnp-org:service:ContentDirectory:1"><ObjectID>{object}</ObjectID><BrowseFlag>{flag}</BrowseFlag><Filter>*</Filter><StartingIndex>0</StartingIndex><RequestedCount>0</RequestedCount><SortCriteria></SortCriteria></u:Browse></s:Body></s:Envelope>"#
    );
    Request::builder()
        .uri(format!("/dlna/{secret}/control/cds"))
        .method("POST")
        .header("host", "192.168.1.5:3000")
        .header("content-type", "text/xml; charset=\"utf-8\"")
        .header(
            "soapaction",
            "\"urn:schemas-upnp-org:service:ContentDirectory:1#Browse\"",
        )
        .body(Body::from(body))
        .unwrap()
}

#[tokio::test]
async fn content_directory_follows_the_key() {
    let dir = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&dir.path().join("dlna.sqlite"))
        .await
        .unwrap();
    store
        .bootstrap("admin", Zeroizing::new(PASSWORD.into()))
        .await
        .unwrap();
    let root = dir.path().join("media");
    std::fs::create_dir_all(&root).unwrap();
    let root = root.to_string_lossy().into_owned();
    for (lib, kind) in [("films", "films"), ("hidden", "films"), ("shows", "series")] {
        query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES(?,?,?,'local',?,1)")
            .bind(lib)
            .bind(format!("Lib {lib}"))
            .bind(kind)
            .bind(&root)
            .execute(store.pool())
            .await
            .unwrap();
    }
    let add = async |work: &str, lib: &str, kind: &str, title: &str| {
        query("INSERT INTO inventory_works(id,library_id,identity_key,title,external_ids_json,kind,title_sort,has_available_asset) VALUES(?,?,?,?,'{}',?,?,1)")
            .bind(work).bind(lib).bind(work).bind(title).bind(kind).bind(title)
            .execute(store.pool()).await.unwrap();
        if kind == "series" {
            return;
        }
        for (table, id, parent) in [
            ("inventory_editions", format!("e-{work}"), "work_id"),
            ("inventory_versions", format!("v-{work}"), "edition_id"),
        ] {
            let parent_id = if parent == "work_id" {
                work.to_owned()
            } else {
                format!("e-{work}")
            };
            query(sqlx::AssertSqlSafe(format!(
                "INSERT INTO {table}(id,{parent}) VALUES(?,?)"
            )))
            .bind(id)
            .bind(parent_id)
            .execute(store.pool())
            .await
            .unwrap();
        }
        query(
            "INSERT INTO inventory_sources(root_snapshot,id,version_id,library_id) VALUES(?,?,?,?)",
        )
        .bind(&root)
        .bind(format!("s-{work}"))
        .bind(format!("v-{work}"))
        .bind(lib)
        .execute(store.pool())
        .await
        .unwrap();
        query("INSERT INTO inventory_assets(id,source_id,library_id,relative_path,size,modified_ns,mime,probe_json,available) VALUES(?,?,?,?,4,'0','video/x-matroska',?,1)")
            .bind(format!("a-{work}")).bind(format!("s-{work}")).bind(lib).bind(format!("{work}.mkv"))
            .bind(r#"{"format":{"duration":"60.000"},"streams":[{"index":0,"codec_type":"video","codec_name":"h264","width":1280,"height":720}],"chapters":[]}"#)
            .execute(store.pool()).await.unwrap();
    };
    add("film-1", "films", "film", "Le Film & Co").await;
    add("secret-1", "hidden", "film", "Interdit").await;
    add("show-1", "shows", "series", "La Série").await;
    add("ep-1", "shows", "episode", "Pilote").await;
    query(
        "INSERT INTO inventory_seasons(id,series_id,season_number) VALUES('season-1','show-1',1)",
    )
    .execute(store.pool())
    .await
    .unwrap();
    query("INSERT INTO inventory_episodes(id,season_id,episode_number,work_id) VALUES('e1','season-1',1,'ep-1')").execute(store.pool()).await.unwrap();

    // A user who reads "films" and "shows" (not "hidden"), and a key for both.
    let state = AuthState::new(store.clone(), ORIGIN.into()).await.unwrap();
    let app = auth::router(state.clone())
        .merge(admin_users::router(state.clone()))
        .merge(api_keys::router(state));
    let login = json_req(
        &app,
        "/api/v1/auth/login",
        "POST",
        "",
        "",
        json!({"login":"admin","password":PASSWORD}),
    )
    .await;
    let cookie = login.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let csrf: Value = serde_json::from_str(&text(login).await).unwrap();
    let csrf = csrf["csrfToken"].as_str().unwrap().to_owned();
    let user = json_req(
        &app,
        "/api/v1/admin/users",
        "POST",
        &cookie,
        &csrf,
        json!({"login":"salon","password":PASSWORD,"role":"user"}),
    )
    .await;
    let user: Value = serde_json::from_str(&text(user).await).unwrap();
    let user = user["id"].as_str().unwrap().to_owned();
    for lib in ["films", "shows"] {
        query("INSERT INTO library_permissions(library_id,user_id) VALUES(?,?)")
            .bind(lib)
            .bind(&user)
            .execute(store.pool())
            .await
            .unwrap();
    }
    let key = json_req(
        &app,
        "/api/v1/admin/api-keys",
        "POST",
        &cookie,
        &csrf,
        json!({"name":"DLNA","userId":user,"libraryIds":["films","shows"],"expiresInDays":30}),
    )
    .await;
    assert_eq!(key.status(), StatusCode::CREATED);
    let key: Value = serde_json::from_str(&text(key).await).unwrap();
    let config = DlnaConfig::new(
        key["token"].as_str().unwrap().to_owned(),
        "Salon".into(),
        3000,
    );
    let secret = config.secret.clone();
    let dlna = router(store.clone(), config);

    // Wrong secret: nothing, not even the description.
    let wrong = Request::builder()
        .uri("/dlna/0123456789abcdef0123456789abcdef/device.xml")
        .body(Body::empty())
        .unwrap();
    assert_eq!(send(&dlna, wrong).await.status(), StatusCode::NOT_FOUND);
    let description = Request::builder()
        .uri(format!("/dlna/{secret}/device.xml"))
        .body(Body::empty())
        .unwrap();
    let description = text(send(&dlna, description).await).await;
    assert!(
        description.contains("<friendlyName>Salon</friendlyName>"),
        "{description}"
    );

    let top = text(send(&dlna, browse(&secret, "0", "BrowseDirectChildren")).await).await;
    assert!(
        top.contains("Lib films") && top.contains("Lib shows"),
        "{top}"
    );
    assert!(!top.contains("Lib hidden"), "{top}");
    assert!(top.contains("<TotalMatches>2</TotalMatches>"), "{top}");

    let films = text(send(&dlna, browse(&secret, "l:films", "BrowseDirectChildren")).await).await;
    // DIDL is itself escaped inside the SOAP Result.
    assert!(films.contains("Le Film &amp;amp; Co"), "{films}");
    assert!(films.contains("http://192.168.1.5:3000/dlna/"), "{films}");
    assert!(films.contains("/media/a-film-1"), "{films}");
    assert!(
        films.contains("duration=&quot;0:01:00.000&quot;"),
        "{films}"
    );

    let shows = text(send(&dlna, browse(&secret, "l:shows", "BrowseDirectChildren")).await).await;
    assert!(
        shows.contains("s:show-1") && !shows.contains("Pilote"),
        "{shows}"
    );
    let seasons =
        text(send(&dlna, browse(&secret, "s:show-1", "BrowseDirectChildren")).await).await;
    assert!(
        seasons.contains("Saison 1") && seasons.contains("n:season-1"),
        "{seasons}"
    );
    let episodes =
        text(send(&dlna, browse(&secret, "n:season-1", "BrowseDirectChildren")).await).await;
    assert!(
        episodes.contains("Pilote") && episodes.contains("/media/a-ep-1"),
        "{episodes}"
    );
    let metadata = text(send(&dlna, browse(&secret, "w:ep-1", "BrowseMetadata")).await).await;
    assert!(
        metadata.contains("parentID=&quot;n:season-1&quot;"),
        "{metadata}"
    );

    // The file itself, with a byte range as TVs seek.
    std::fs::write(std::path::Path::new(&root).join("film-1.mkv"), b"MKV!").unwrap();
    let file = Request::builder()
        .uri(format!("/dlna/{secret}/media/a-film-1"))
        .header("range", "bytes=1-")
        .body(Body::empty())
        .unwrap();
    let file = send(&dlna, file).await;
    let status = file.status();
    if status != StatusCode::PARTIAL_CONTENT {
        panic!("{status} {:?}", text(file).await);
    }
    assert_eq!(file.headers()["transfermode.dlna.org"], "Streaming");
    assert_eq!(text(file).await, "KV!");

    // Another library's work and its file are refused.
    let refused = send(&dlna, browse(&secret, "w:secret-1", "BrowseMetadata")).await;
    assert_eq!(refused.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert!(text(refused).await.contains("<errorCode>701</errorCode>"));
    let file = Request::builder()
        .uri(format!("/dlna/{secret}/media/a-secret-1"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(send(&dlna, file).await.status(), StatusCode::NOT_FOUND);

    // Revoking the key turns DLNA off at once.
    let path = format!("/api/v1/admin/api-keys/{}", key["id"].as_str().unwrap());
    assert_eq!(
        json_req(&app, &path, "DELETE", &cookie, &csrf, json!(null))
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    let after = text(send(&dlna, browse(&secret, "0", "BrowseDirectChildren")).await).await;
    assert!(after.contains("<errorCode>701</errorCode>"), "{after}");
    let file = Request::builder()
        .uri(format!("/dlna/{secret}/media/a-film-1"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(send(&dlna, file).await.status(), StatusCode::NOT_FOUND);
}
