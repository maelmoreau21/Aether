use crate::node::Inventory;
use axum::{Json, extract::State};
use std::sync::Arc;

pub async fn media(State(inventory): State<Arc<Inventory>>) -> Json<serde_json::Value> {
    let assets = inventory.demo_descriptors();
    let media: Vec<_> = ["mkv", "mp4"].into_iter().filter_map(|container| {
        let sources: Vec<_> = ["fr", "en"].into_iter().filter_map(|language| {
            let id = format!("demo-{container}-{language}");
            assets.iter().find(|asset| asset.id == id).map(|asset| serde_json::json!({
                "id":id, "audioTrackId":format!("audio-{language}"), "language":language,
                "url":format!("/api/v1/assets/{id}/content"), "mime":asset.mime,
                "tracks":asset.probe.streams
            }))
        }).collect();
        if sources.len() != 2 { return None; }
        Some(serde_json::json!({
            "id":format!("demo-{container}"), "title":format!("Mire AV1 10 bits · {}", container.to_uppercase()), "synthetic":true,
            "sources":sources,
            "subtitles":[
                {"id":"fr-forced", "language":"fr", "label":"Français forcés", "url":"/api/v1/demo/subtitles/fr-forced.vtt"},
                {"id":"fr-full", "language":"fr", "label":"Français", "url":"/api/v1/demo/subtitles/fr-full.vtt"},
                {"id":"en-full", "language":"en", "label":"English", "url":"/api/v1/demo/subtitles/en-full.vtt"}
            ]
        }))
    }).collect();
    Json(
        serde_json::json!({"media":media, "notice":"Fixtures synthétiques locales uniquement. Préparer scripts/prepare-p1.py si la liste est vide."}),
    )
}
