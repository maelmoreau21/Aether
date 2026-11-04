//! Conservative recording acquisition: only a proven, one-track Single release.
//! Proof is constructed by the host, never accepted from the browser.
use super::*;
use std::collections::BTreeSet;
pub(super) struct Resolution {
    pub selection: RequestInput,
    pub canonical: Value,
    recording: String,
    releases: BTreeSet<String>,
    selected_release: String,
}
fn unsupported() -> Error {
    err(StatusCode::UNPROCESSABLE_ENTITY, "track_not_supported")
}
fn identifier(value: &Value) -> Result<String, Error> {
    let id = value.as_str().ok_or_else(unsupported)?;
    uuid::Uuid::parse_str(id).map_err(|_| unsupported())?;
    Ok(id.to_owned())
}
async fn metadata(path: &str, inc: &str) -> Result<Value, Error> {
    metadata_query(path, inc, &[]).await
}
async fn metadata_query(path: &str, inc: &str, extra: &[(&str, String)]) -> Result<Value, Error> {
    static LAST: tokio::sync::Mutex<Option<tokio::time::Instant>> =
        tokio::sync::Mutex::const_new(None);
    let mut last = LAST.lock().await;
    if let Some(previous) = *last {
        tokio::time::sleep_until(previous + std::time::Duration::from_millis(1100)).await;
    }
    *last = Some(tokio::time::Instant::now());
    let connection = transport::configure("https://musicbrainz.org/", "").await?;
    let mut params = vec![("fmt", "json".into()), ("inc", inc.into())];
    params.extend_from_slice(extra);
    transport::request(&connection, "GET", path, &params, None).await
}
fn group(recording: &Value) -> Result<String, Error> {
    let releases = recording["releases"].as_array().ok_or_else(unsupported)?;
    if releases.len() > 100
        || recording["release-count"]
            .as_u64()
            .is_some_and(|n| n > releases.len() as u64)
    {
        return Err(unsupported());
    }
    let groups: BTreeSet<_> = releases
        .iter()
        .filter(|r| r["release-group"]["primary-type"] == "Single")
        .map(|r| identifier(&r["release-group"]["id"]))
        .collect::<Result<_, _>>()?;
    if groups.len() != 1 {
        return Err(unsupported());
    }
    groups.into_iter().next().ok_or_else(unsupported)
}
fn release_matches(release: &Value, group: &str, recording: &str) -> bool {
    if release["release-group"]["id"] != group
        || release["release-group"]["primary-type"] != "Single"
    {
        return false;
    }
    let Some(media) = release["media"].as_array() else {
        return false;
    };
    let mut count = 0usize;
    for medium in media {
        let Some(tracks) = medium["tracks"].as_array() else {
            return false;
        };
        if medium["track-count"].as_u64() != Some(tracks.len() as u64) {
            return false;
        }
        for track in tracks {
            count += 1;
            if track["recording"]["id"] != recording {
                return false;
            }
        }
    }
    count == 1
}
fn album_releases(
    album: &Value,
    verified: Option<&BTreeSet<String>>,
) -> Result<BTreeSet<String>, Error> {
    if album["albumType"]
        .as_str()
        .is_none_or(|s| !s.eq_ignore_ascii_case("Single"))
    {
        return Err(unsupported());
    }
    let releases = album["releases"].as_array().ok_or_else(unsupported)?;
    if releases.is_empty() || releases.len() > 4 {
        return Err(unsupported());
    }
    releases
        .iter()
        .map(|r| {
            if r["trackCount"].as_u64() != Some(1) {
                return Err(unsupported());
            }
            let id = identifier(&r["foreignReleaseId"])?;
            if verified.is_some_and(|known| !known.contains(&id)) {
                return Err(unsupported());
            }
            Ok(id)
        })
        .collect()
}
pub(super) async fn resolve(server: &Server, input: &RequestInput) -> Result<Resolution, Error> {
    uuid::Uuid::parse_str(&input.external_id).map_err(|_| unsupported())?;
    let recording = metadata(
        &format!("ws/2/recording/{}", input.external_id),
        "releases+release-groups",
    )
    .await?;
    if recording["id"] != input.external_id {
        return Err(unsupported());
    }
    let releases = metadata_query(
        "ws/2/release",
        "release-groups",
        &[
            ("recording", input.external_id.clone()),
            ("limit", "100".into()),
            ("offset", "0".into()),
        ],
    )
    .await?;
    if releases["release-count"].as_u64().is_none() {
        return Err(unsupported());
    }
    let group = group(&releases)?;
    let mut selection = input.clone();
    selection.kind = "album".into();
    selection.external_id = group.clone();
    let mut canonical = remote::lookup(server, &selection).await?;
    let releases = album_releases(&canonical, None)?;
    for release in &releases {
        let detail = metadata(
            &format!("ws/2/release/{release}"),
            "recordings+release-groups",
        )
        .await?;
        if detail["id"] != *release || !release_matches(&detail, &group, &input.external_id) {
            return Err(unsupported());
        }
    }
    let selected_release = releases.first().cloned().ok_or_else(unsupported)?;
    canonical["_singleRecordingId"] = json!(input.external_id);
    canonical["_singleReleaseId"] = json!(selected_release);
    Ok(Resolution {
        selection,
        canonical,
        recording: input.external_id.clone(),
        releases,
        selected_release,
    })
}
impl Resolution {
    pub fn proof(&self) -> Value {
        json!({"resolvedSingleId":self.selection.external_id,"verifiedRecordingId":self.recording})
    }
    /// Pin an exact release before enabling acquisition. Existing unrelated release
    /// monitoring is never overwritten; unverifiable refreshed metadata fails closed.
    pub async fn prepare(&self, server: &Server, id: &str) -> Result<(), Error> {
        let path = format!("album/{id}");
        let mut album = server.call("GET", &path, &[], None).await?;
        if album["foreignAlbumId"] != self.selection.external_id {
            return Err(unsupported());
        }
        let available = album_releases(&album, Some(&self.releases))?;
        if !available.contains(&self.selected_release) {
            return Err(unsupported());
        }
        if album["monitored"] == true && album["anyReleaseOk"] == true {
            return Err(err(
                StatusCode::UNPROCESSABLE_ENTITY,
                "single_monitoring_conflict",
            ));
        }
        let was_monitored = album["monitored"] == true;
        album["anyReleaseOk"] = json!(false);
        if let Some(releases) = album["releases"].as_array_mut() {
            for release in releases {
                release["monitored"] = json!(release["foreignReleaseId"] == self.selected_release);
            }
        }
        // A newly created single stays unmonitored until the persisted identity
        // and release selection have both been read back and checked.
        album["monitored"] = json!(was_monitored);
        server.call("PUT", &path, &[], Some(album)).await?;
        let mut readback = server.call("GET", &path, &[], None).await?;
        if readback["anyReleaseOk"] != false {
            return Err(unsupported());
        }
        let selected: Vec<_> = readback["releases"]
            .as_array()
            .ok_or_else(unsupported)?
            .iter()
            .filter(|r| r["monitored"] == true)
            .collect();
        if selected.len() != 1
            || selected[0]["foreignReleaseId"] != self.selected_release
            || selected[0]["trackCount"] != 1
        {
            return Err(unsupported());
        }
        let tracks = server
            .call("GET", "track", &[("albumId", id.into())], None)
            .await?;
        let tracks = tracks.as_array().ok_or_else(unsupported)?;
        if tracks.len() != 1 || tracks[0]["foreignRecordingId"] != self.recording {
            return Err(unsupported());
        }
        if !was_monitored {
            readback["monitored"] = json!(true);
            server.call("PUT", &path, &[], Some(readback)).await?;
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    const GROUP: &str = "12345678-1234-1234-1234-123456789abc";
    const RECORDING: &str = "22345678-1234-1234-1234-123456789abc";
    #[test]
    fn browser_input_cannot_supply_single_proof() {
        assert!(serde_json::from_value::<RequestInput>(json!({"kind":"track","title":"Synthetic","externalId":RECORDING,"externalProvider":"musicbrainz","resolvedSingleId":GROUP,"verifiedRecordingId":RECORDING})).is_err());
    }
    #[tokio::test]
    async fn lidarr_must_read_back_the_pinned_single_and_exact_recording() {
        const RELEASE: &str = "32345678-1234-1234-1234-123456789abc";
        let album = json!({"id":7,"foreignAlbumId":GROUP,"albumType":"Single","monitored":false,"anyReleaseOk":false,"releases":[{"id":9,"foreignReleaseId":RELEASE,"trackCount":1,"monitored":true}]});
        let mock = crate::acquisition::remote_tests::mock(
            "lidarr",
            vec![
                ("GET /api/v1/album/7", vec![(StatusCode::OK, album)]),
                ("PUT /api/v1/album/7", vec![(StatusCode::OK, json!({}))]),
                (
                    "GET /api/v1/track",
                    vec![(StatusCode::OK, json!([{"foreignRecordingId":RECORDING}]))],
                ),
            ],
        )
        .await;
        let selection:RequestInput=serde_json::from_value(json!({"kind":"album","title":"Synthetic","externalId":GROUP,"externalProvider":"musicbrainz"})).unwrap();
        let resolution = Resolution {
            selection,
            canonical: json!({}),
            recording: RECORDING.into(),
            releases: BTreeSet::from([RELEASE.into()]),
            selected_release: RELEASE.into(),
        };
        resolution.prepare(&mock.server, "7").await.unwrap();
    }
    #[test]
    fn exact_single_proof_rejects_missing_ambiguous_and_album_candidates() {
        assert!(group(&json!({"releases":[]})).is_err());
        assert!(
            group(&json!({"releases":[{"release-group":{"id":GROUP,"primary-type":"Album"}}]}))
                .is_err()
        );
        assert_eq!(
            group(&json!({"releases":[{"release-group":{"id":GROUP,"primary-type":"Single"}}]}))
                .unwrap(),
            GROUP
        );
        assert!(group(&json!({"release-count":2,"releases":[{"release-group":{"id":GROUP,"primary-type":"Single"}}]})).is_err());
        assert!(group(&json!({"releases":[{"release-group":{"id":GROUP,"primary-type":"Single"}},{"release-group":{"id":RECORDING,"primary-type":"Single"}}]})).is_err());
    }
    #[test]
    fn a_single_release_must_contain_exactly_the_requested_recording() {
        let mut release = json!({"release-group":{"id":GROUP,"primary-type":"Single"},"media":[{"track-count":1,"tracks":[{"recording":{"id":RECORDING}}]}]});
        assert!(release_matches(&release, GROUP, RECORDING));
        release["media"][0]["track-count"] = json!(2);
        assert!(!release_matches(&release, GROUP, RECORDING));
        release["media"][0]["track-count"] = json!(1);
        release["media"][0]["tracks"][0]["recording"]["id"] = json!(GROUP);
        assert!(!release_matches(&release, GROUP, RECORDING));
    }
}
