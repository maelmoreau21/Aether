//! Opt-in real external guest against deterministic responses in the production Wasmi budget.
//! No network, account, signed package installation, or sibling workspace dependency in CI.
use super::*;
use serde_json::{Value, json};

struct OfflineResponses;
impl Drop for OfflineResponses {
    fn drop(&mut self) {
        TEST_HTTP_SEQUENCE.with(|queue| *queue.borrow_mut() = None);
    }
}

fn invoke(wasm: &[u8], operation: &str, mut input: Value, responses: Vec<(&str, Value)>) -> Value {
    input["config"] = json!({"api_key":"synthetic-project-key","access_model":"licensed","fallback_language":"en"});
    let _guard = OfflineResponses;
    TEST_HTTP_SEQUENCE.with(|queue| {
        *queue.borrow_mut() = Some(
            responses
                .into_iter()
                .map(|(endpoint, data)| {
                    (
                        format!("https://api4.thetvdb.com/v4/{endpoint}"),
                        serde_json::to_vec(&json!({"status":"success","data":data})).unwrap(),
                    )
                })
                .collect(),
        );
    });
    let mut invocation = Invocation::new(
        STANDARD.encode(wasm),
        ExtensionRequest {
            api_version: 1,
            operation: operation.into(),
            input,
        },
    );
    invocation.allowed_hosts = vec!["api4.thetvdb.com".into(), "artworks.thetvdb.com".into()];
    let result = execute(invocation)
        .expect("TVDB guest must fit actual Core Wasmi limits")
        .result;
    TEST_HTTP_SEQUENCE.with(|queue| {
        assert!(
            queue.borrow().as_ref().unwrap().is_empty(),
            "guest did not consume the expected provider responses"
        )
    });
    assert!(
        result.get("error").is_none(),
        "guest returned an error: {result}"
    );
    result
}

#[test]
#[ignore = "requires MEDIA_TEST_TVDB_WASM pointing to the optimized external TVDB guest"]
fn tvdb_guest_search_details_and_images_fit_core_wasmi_budget() {
    let wasm =
        std::fs::read(std::env::var_os("MEDIA_TEST_TVDB_WASM").expect("set MEDIA_TEST_TVDB_WASM"))
            .unwrap();
    assert!(wasm.len() <= MAX_MODULE);
    let login = json!({"token":"synthetic-private-token"});
    // Twenty ordinary populated results, not an empty-provider success test.
    let items: Vec<_> = (1..=20).map(|id| json!({"tvdb_id":id.to_string(),"type":"movie","name":format!("Synthetic film {id}"),"overview":"Synthetic plot. ".repeat(80)})).collect();
    let search = invoke(
        &wasm,
        "metadata.search",
        json!({"title":"Synthetic","mediaType":"movie"}),
        vec![
            ("login", login.clone()),
            ("search?limit=20&query=Synthetic&type=movie", json!(items)),
        ],
    );
    assert_eq!(search.as_array().unwrap().len(), 20);
    let record = json!({"id":42,"name":"Synthetic movie","overview":"Synthetic plot. ".repeat(80),"year":"2024","remoteIds":[{"sourceName":"IMDB","id":"tt0042"}],"artworks":[{"type":9,"image":"https://artworks.thetvdb.com/poster.jpg","language":"eng"}]});
    let details = invoke(
        &wasm,
        "metadata.details",
        json!({"id":"42","mediaType":"movie"}),
        vec![
            ("login", login.clone()),
            ("movies/42/extended", record.clone()),
            (
                "movies/42/translations/eng",
                json!({"name":"Synthetic movie","overview":"English translation"}),
            ),
        ],
    );
    assert_eq!(details["providerId"], "42");
    assert_eq!(details["externalIds"]["imdb"], "tt0042");
    let images = invoke(
        &wasm,
        "metadata.images",
        json!({"id":"42","mediaType":"movie"}),
        vec![
            ("login", login),
            ("movies/42/extended", record),
            ("artwork/types", json!([{"id":9,"name":"Movie Poster"}])),
        ],
    );
    assert_eq!(images.as_array().unwrap().len(), 1);
    assert_eq!(images[0]["imageType"], "poster");
}
