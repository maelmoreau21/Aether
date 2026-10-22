//! Admission contract: a cold client must serialize independent preparation requests.
use media_node::{
    files::OpenedFile,
    packaging::{Component, Preparations},
};
use std::path::Path;

#[tokio::test]
async fn concurrent_cold_components_reject_one_request_but_serial_retry_succeeds() {
    let root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/generated/representative");
    for extension in ["mkv", "mp4"] {
        let name = format!("representative.{extension}");
        let relative = Path::new(&name);
        let original = OpenedFile::open_in_root(&root, relative).await.unwrap();
        let preparations = Preparations::default();
        let (video, audio) = tokio::join!(
            preparations.prepare(
                &root,
                relative,
                original.size(),
                original.modified_ns(),
                Component::Video
            ),
            preparations.prepare(
                &root,
                relative,
                original.size(),
                original.modified_ns(),
                Component::Audio(1)
            )
        );
        // Opening the input is asynchronous: either component may acquire the worker first.
        assert_ne!(video.is_ok(), audio.is_ok());
        for result in [&video, &audio] {
            if let Err(error) = result {
                assert_eq!(error.to_string(), "preparation busy; retry explicitly");
            }
        }
        preparations
            .prepare(
                &root,
                relative,
                original.size(),
                original.modified_ns(),
                Component::Video,
            )
            .await
            .unwrap();
        let audio = preparations
            .prepare(
                &root,
                relative,
                original.size(),
                original.modified_ns(),
                Component::Audio(1),
            )
            .await
            .unwrap();
        let probe = media_node::probe::probe_opened(&audio).await.unwrap();
        assert_eq!(
            probe.streams[0].details.get("channels"),
            Some(&serde_json::json!(6))
        );
        assert_eq!(
            probe.streams[0].details.get("channels"),
            original_audio_channels(&original).await.as_ref()
        );
    }
}

async fn original_audio_channels(original: &OpenedFile) -> Option<serde_json::Value> {
    media_node::probe::probe_opened(original)
        .await
        .unwrap()
        .streams
        .into_iter()
        .find(|s| s.index == 1)
        .unwrap()
        .details
        .get("channels")
        .cloned()
}
