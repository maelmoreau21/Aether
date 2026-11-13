//! Executable contracts for uncertain creation, precise monitoring and inventory proof.
use super::*;
use axum::extract::{Request, State};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
};
use tokio::sync::Mutex;
type Replies = BTreeMap<String, VecDeque<(StatusCode, Value)>>;
#[derive(Default)]
struct MockState {
    replies: Replies,
    calls: Vec<(String, Value)>,
}
pub(super) struct Mock {
    pub(super) server: Server,
    state: Arc<Mutex<MockState>>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Mock {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn handle(
    State(state): State<Arc<Mutex<MockState>>>,
    request: Request,
) -> (StatusCode, Json<Value>) {
    let key = format!("{} {}", request.method(), request.uri().path());
    let bytes = axum::body::to_bytes(request.into_body(), 65536)
        .await
        .unwrap();
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    let mut state = state.lock().await;
    state.calls.push((key.clone(), body));
    let reply = state.replies.get_mut(&key).and_then(|q| {
        if q.len() > 1 {
            q.pop_front()
        } else {
            q.front().cloned()
        }
    });
    let (status, body) = reply.unwrap_or((
        StatusCode::NOT_FOUND,
        json!({"error":"unexpected_fixture_request"}),
    ));
    (status, Json(body))
}
pub(super) async fn mock(kind: &str, replies: Vec<(&str, Vec<(StatusCode, Value)>)>) -> Mock {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let state = Arc::new(Mutex::new(MockState {
        replies: replies
            .into_iter()
            .map(|(key, values)| (key.into(), values.into()))
            .collect(),
        calls: vec![],
    }));
    let app = Router::new().fallback(handle).with_state(state.clone());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let server = Server {
        id: "mock".into(),
        name: "Fixture".into(),
        kind: kind.into(),
        base_url: format!("http://{address}/"),
        api_key: "fixture-key".into(),
        allowed_ips_json: "[\"127.0.0.1\"]".into(),
        library_id: "request-films".into(),
        root_folder: "/movies".into(),
        quality_profile_id: 1,
        quality_profiles_json: "[]".into(),
        metadata_profile_id: None,
        is_default: 1,
        enabled: 1,
        generation: "test".into(),
        guard: None,
    };
    Mock {
        server,
        state,
        task,
    }
}
fn input(kind: &str) -> RequestInput {
    RequestInput {
        kind: kind.into(),
        title: "Synthetic".into(),
        external_id: "123".into(),
        external_provider: if kind == "series" { "tvdb" } else { "tmdb" }.into(),
        server_id: None,
        quality_profile_id: None,
        seasons: vec![],
        episodes: vec![],
        album_ids: vec![],
        monitor_future: false,
    }
}
#[tokio::test]
async fn uncertain_creation_reconciles_identity_without_a_second_post() {
    let movie = json!({"id":7,"tmdbId":123,"title":"Synthetic"});
    let mock = mock(
        "radarr",
        vec![
            (
                "GET /api/v3/movie",
                vec![
                    (StatusCode::OK, json!([])),
                    (StatusCode::OK, json!([movie])),
                ],
            ),
            (
                "POST /api/v3/movie",
                vec![(StatusCode::INTERNAL_SERVER_ERROR, json!({}))],
            ),
        ],
    )
    .await;
    let canonical = json!({"tmdbId":123,"title":"Synthetic"});
    assert!(
        remote::ensure(&mock.server, &input("film"), &canonical)
            .await
            .is_err()
    );
    assert_eq!(
        remote::ensure(&mock.server, &input("film"), &canonical)
            .await
            .unwrap(),
        "7"
    );
    assert_eq!(
        mock.state
            .lock()
            .await
            .calls
            .iter()
            .filter(|(key, _)| key == "POST /api/v3/movie")
            .count(),
        1
    );
}
#[tokio::test]
async fn selecting_a_season_preserves_other_monitoring_and_future_policy() {
    let mock=mock("sonarr",vec![("GET /api/v3/series/7",vec![(StatusCode::OK,json!({"id":7,"monitorNewItems":"all","seasons":[{"seasonNumber":1,"monitored":false},{"seasonNumber":2,"monitored":true},{"seasonNumber":3,"monitored":false}]}))]),("PUT /api/v3/series/7",vec![(StatusCode::OK,json!({}))]),("POST /api/v3/command",vec![(StatusCode::OK,json!({}))])]).await;
    let mut selection = input("series");
    selection.seasons = vec![1];
    remote::search_missing(&mock.server, &selection, "7")
        .await
        .unwrap();
    let state = mock.state.lock().await;
    let updated = &state
        .calls
        .iter()
        .find(|(key, _)| key == "PUT /api/v3/series/7")
        .unwrap()
        .1;
    assert_eq!(updated["seasons"][0]["monitored"], true);
    assert_eq!(updated["seasons"][1]["monitored"], true);
    assert_eq!(updated["seasons"][2]["monitored"], false);
    assert_eq!(updated["monitorNewItems"], "all");
    let commands = state
        .calls
        .iter()
        .filter(|(key, _)| key == "POST /api/v3/command")
        .collect::<Vec<_>>();
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].1["seasonNumber"], 1);
}
#[tokio::test]
async fn multi_episode_file_counts_once_and_requires_current_node_inventory() {
    let path = "/movies/Show/S01E01-E02.mkv";
    let mock=mock("sonarr",vec![("GET /api/v3/episode",vec![(StatusCode::OK,json!([{"seasonNumber":1,"episodeNumber":1,"episodeFileId":8,"hasFile":true},{"seasonNumber":1,"episodeNumber":2,"episodeFileId":8,"hasFile":true}]))]),("GET /api/v3/episodefile",vec![(StatusCode::OK,json!([{"id":8,"path":path}]))])]).await;
    let progress = remote::progress(&mock.server, &input("series"), "7")
        .await
        .unwrap();
    assert!(progress.imported);
    assert_eq!(progress.expected, 1);
    let (_dir, store, _, _) = tests::fixture().await;
    assert_eq!(
        worker::indexed(&store, &mock.server, "series", &progress.paths)
            .await
            .unwrap()
            .0,
        0
    );
    for sql in [
        "INSERT INTO inventory_works(id,library_id,identity_key,title,external_ids_json) VALUES('work','request-films','synthetic','Synthetic','{}')",
        "INSERT INTO inventory_editions(id,work_id) VALUES('edition','work')",
        "INSERT INTO inventory_versions(id,edition_id) VALUES('version','edition')",
        "INSERT INTO inventory_sources(id,version_id,library_id,root_snapshot) VALUES('source','version','request-films','/old-root')",
        "INSERT INTO inventory_assets(id,source_id,library_id,relative_path,size,modified_ns,mime,probe_json,available) VALUES('asset','source','request-films','Show/S01E01-E02.mkv',1,'1','video/x-matroska','{}',1)",
    ] {
        query(sql).execute(store.pool()).await.unwrap();
    }
    assert_eq!(
        worker::indexed(&store, &mock.server, "series", &progress.paths)
            .await
            .unwrap()
            .0,
        0
    );
    query("UPDATE inventory_sources SET root_snapshot='/media'")
        .execute(store.pool())
        .await
        .unwrap();
    let (count, work) =
        worker::indexed(&store, &mock.server, "series", &[path.into(), path.into()])
            .await
            .unwrap();
    assert_eq!(count, 1);
    assert_eq!(work.as_deref(), Some("work"));
}
#[tokio::test]
async fn stored_numeric_id_cannot_target_a_different_remote_work() {
    let mock = mock(
        "radarr",
        vec![(
            "GET /api/v3/movie/7",
            vec![(StatusCode::OK, json!({"id":7,"tmdbId":999}))],
        )],
    )
    .await;
    assert_eq!(
        remote::verify_identity(&mock.server, &input("film"), "7")
            .await
            .unwrap_err()
            .1
            .0["error"],
        "remote_content_identity_changed"
    );
    assert_eq!(mock.state.lock().await.calls.len(), 1);
}
