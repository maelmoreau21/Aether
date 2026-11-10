//! REST adapters; no remote API key or filesystem path originates in a user request.
use super::*;

fn array(value: &Value) -> Result<&Vec<Value>, Error> {
    value
        .as_array()
        .ok_or_else(|| err(StatusCode::BAD_GATEWAY, "invalid_remote_response"))
}
fn external(v: &Value, kind: &str) -> Option<String> {
    match kind {
        "film" => v["tmdbId"].as_i64().map(|v| v.to_string()),
        "series" => v["tvdbId"].as_i64().map(|v| v.to_string()),
        "artist" => v["foreignArtistId"].as_str().map(str::to_owned),
        "album" => v["foreignAlbumId"].as_str().map(str::to_owned),
        _ => None,
    }
}
pub fn title<'a>(v: &'a Value, kind: &str) -> Option<&'a str> {
    if kind == "artist" {
        v["artistName"].as_str()
    } else {
        v["title"].as_str()
    }
}
fn endpoint(kind: &str) -> &str {
    match kind {
        "film" => "movie",
        "series" => "series",
        "artist" => "artist",
        _ => "album",
    }
}
pub async fn search(server: &Server, kind: &str, q: &str) -> Result<Vec<Value>, Error> {
    if kind == "track" {
        let connection = transport::configure("https://musicbrainz.org/", "").await?;
        let result = transport::request(
            &connection,
            "GET",
            "ws/2/recording",
            &[
                ("query", q.into()),
                ("fmt", "json".into()),
                ("limit", "25".into()),
            ],
            None,
        )
        .await?;
        let values = result["recordings"]
            .as_array()
            .ok_or_else(|| err(StatusCode::BAD_GATEWAY, "invalid_remote_response"))?;
        return Ok(values.iter().filter_map(|v|Some(json!({"kind":"track","title":v["title"].as_str()?,"externalId":v["id"].as_str()?,"externalProvider":"musicbrainz","serverId":server.id,"artist":v["artist-credit"][0]["name"],"album":v["releases"][0]["title"],"year":v["first-release-date"].as_str().and_then(|d|d.get(..4)).and_then(|y|y.parse::<i64>().ok()),"acquisitionSupported":false}))).collect());
    }
    let results = server
        .call(
            "GET",
            &format!("{}/lookup", endpoint(kind)),
            &[("term", q.into())],
            None,
        )
        .await?;
    Ok(array(&results)?
        .iter()
        .take(50)
        .filter_map(|v| {
            let mut result = json!({"kind":kind,"title":title(v,kind)?,"externalId":external(v,kind)?,
                "externalProvider":match kind{"film"=>"tmdb","series"=>"tvdb",_=>"musicbrainz"},
                "serverId":server.id});
            describe(&mut result, v, kind);
            Some(result)
        })
        .collect())
}
/// Bounded, display-only facts from a lookup result. Remote image URLs are not
/// forwarded: the interface only loads images served by Aether.
fn describe(result: &mut Value, v: &Value, kind: &str) {
    let text = |value: &Value, max: usize| {
        value.as_str().map(|s| {
            s.chars()
                .filter(|c| !c.is_control() || *c == '\n')
                .take(max)
                .collect::<String>()
        })
    };
    let year = v["year"]
        .as_i64()
        .filter(|y| (1800..=3000).contains(y))
        .or_else(|| {
            v["releaseDate"]
                .as_str()
                .and_then(|d| d.get(..4)?.parse().ok())
                .filter(|y: &i64| (1800..=3000).contains(y))
        });
    result["year"] = json!(year);
    result["summary"] = json!(text(&v["overview"], 600));
    let genres: Vec<String> = v["genres"]
        .as_array()
        .map(|g| g.iter().filter_map(|g| text(g, 40)).take(3).collect())
        .unwrap_or_default();
    result["genres"] = json!(genres);
    result["runtime"] = json!(v["runtime"].as_i64().filter(|m| (1..=2000).contains(m)));
    // A positive id means the item is already tracked by this server.
    result["onServer"] = json!(v["id"].as_i64().is_some_and(|id| id > 0));
    match kind {
        "series" => {
            let mut seasons: Vec<Value> = v["seasons"]
                .as_array()
                .map(|seasons| {
                    seasons
                        .iter()
                        .filter_map(|s| {
                            let number = s["seasonNumber"]
                                .as_i64()
                                .filter(|n| (0..=10000).contains(n))?;
                            let episodes = s["statistics"]["totalEpisodeCount"]
                                .as_i64()
                                .filter(|n| *n > 0 && *n <= 100000);
                            Some(json!({"number":number,"episodes":episodes}))
                        })
                        .take(200)
                        .collect()
                })
                .unwrap_or_default();
            seasons.sort_by_key(|s| s["number"].as_i64());
            result["seasons"] = json!(seasons);
            result["network"] = json!(text(&v["network"], 80));
            result["ended"] = json!(v["ended"].as_bool());
        }
        "album" => {
            result["artist"] = json!(text(&v["artist"]["artistName"], 200));
            result["albumType"] = json!(text(&v["albumType"], 40));
        }
        "artist" => {
            result["artistType"] = json!(text(&v["artistType"], 40));
        }
        _ => {}
    }
}
pub async fn lookup(server: &Server, input: &RequestInput) -> Result<Value, Error> {
    if input.kind == "track" {
        if uuid::Uuid::parse_str(&input.external_id).is_err() {
            return Err(err(StatusCode::UNPROCESSABLE_ENTITY, "invalid_external_id"));
        }
        let connection = transport::configure("https://musicbrainz.org/", "").await?;
        return transport::request(
            &connection,
            "GET",
            &format!("ws/2/recording/{}", input.external_id),
            &[("fmt", "json".into())],
            None,
        )
        .await;
    }
    let prefix = if matches!(input.kind.as_str(), "artist" | "album") {
        "lidarr"
    } else {
        input.external_provider.as_str()
    };
    let result = server
        .call(
            "GET",
            &format!("{}/lookup", endpoint(&input.kind)),
            &[("term", format!("{prefix}:{}", input.external_id))],
            None,
        )
        .await?;
    array(&result)?
        .iter()
        .find(|v| {
            if input.kind == "series" && input.external_provider == "tmdb" {
                v["tmdbId"]
                    .as_i64()
                    .is_some_and(|n| n.to_string() == input.external_id)
            } else {
                external(v, &input.kind).as_deref() == Some(input.external_id.as_str())
            }
        })
        .cloned()
        .ok_or_else(|| err(StatusCode::UNPROCESSABLE_ENTITY, "remote_content_not_found"))
}

fn remote_id(value: &Value) -> Result<String, Error> {
    value["id"]
        .as_i64()
        .filter(|v| *v > 0)
        .map(|v| v.to_string())
        .ok_or_else(|| err(StatusCode::BAD_GATEWAY, "invalid_remote_id"))
}
/// A connection can have been reconfigured after a terminal failure. Never reuse
/// its numeric object id without checking the external identity at that endpoint.
pub async fn verify_identity(server: &Server, input: &RequestInput, id: &str) -> Result<(), Error> {
    let number = id
        .parse::<i64>()
        .ok()
        .filter(|id| *id > 0)
        .ok_or_else(|| err(StatusCode::BAD_GATEWAY, "invalid_remote_id"))?;
    let value = server
        .call(
            "GET",
            &format!("{}/{number}", endpoint(&input.kind)),
            &[],
            None,
        )
        .await?;
    let identity = if input.kind == "series" && input.external_provider == "tmdb" {
        value["tmdbId"].as_i64().map(|id| id.to_string())
    } else {
        external(&value, &input.kind)
    };
    if identity.as_deref() != Some(input.external_id.as_str()) {
        return Err(err(
            StatusCode::UNPROCESSABLE_ENTITY,
            "remote_content_identity_changed",
        ));
    }
    Ok(())
}
pub async fn find(
    server: &Server,
    input: &RequestInput,
    canonical: &Value,
) -> Result<Option<Value>, Error> {
    let query = match input.kind.as_str() {
        "film" => vec![("tmdbId", input.external_id.clone())],
        "series" => vec![("tvdbId", canonical["tvdbId"].to_string())],
        "artist" => vec![("foreignArtistId", input.external_id.clone())],
        "album" => vec![("foreignAlbumId", input.external_id.clone())],
        _ => return Ok(None),
    };
    let result = server
        .call("GET", endpoint(&input.kind), &query, None)
        .await?;
    Ok(array(&result)?
        .iter()
        .find(|v| {
            if input.kind == "series" {
                v["tvdbId"] == canonical["tvdbId"]
            } else {
                external(v, &input.kind) == external(canonical, &input.kind)
            }
        })
        .cloned())
}
/// Ensure one remote object, first reconciling by canonical identity after an uncertain response.
pub async fn ensure(
    server: &Server,
    input: &RequestInput,
    canonical: &Value,
) -> Result<String, Error> {
    if let Some(existing) = find(server, input, canonical).await? {
        return remote_id(&existing);
    }
    let mut body = canonical.clone();
    let single = body["_singleRecordingId"].is_string();
    let single_release = body["_singleReleaseId"].as_str().map(str::to_owned);
    body.as_object_mut()
        .ok_or_else(|| err(StatusCode::BAD_GATEWAY, "invalid_remote_response"))?
        .remove("id");
    if let Some(object) = body.as_object_mut() {
        object.remove("_singleRecordingId");
        object.remove("_singleReleaseId");
    }
    body["rootFolderPath"] = json!(server.root_folder);
    body["qualityProfileId"] = json!(server.quality_profile_id);
    body["monitored"] = json!(!single);
    if single {
        body["anyReleaseOk"] = json!(false);
        if let Some(releases) = body["releases"].as_array_mut() {
            for release in releases {
                release["monitored"] =
                    json!(release["foreignReleaseId"].as_str() == single_release.as_deref());
            }
        }
    }
    match input.kind.as_str() {
        "film" => {
            body["minimumAvailability"] = json!("released");
            body["addOptions"] = json!({"searchForMovie":false});
        }
        "series" => {
            body["seasonFolder"] = json!(true);
            body["monitorNewItems"] = json!(if input.monitor_future { "all" } else { "none" });
            body["addOptions"] = json!({"monitor":"none","searchForMissingEpisodes":false,"searchForCutoffUnmetEpisodes":false});
            if let Some(seasons) = body["seasons"].as_array_mut() {
                for season in seasons {
                    season["monitored"] = json!(false);
                }
            }
        }
        "artist" => {
            body["metadataProfileId"] = json!(server.metadata_profile_id);
            body["monitorNewItems"] = json!(if input.monitor_future { "all" } else { "none" });
            body["addOptions"] = json!({"monitor":"none","albumsToMonitor":input.album_ids,"searchForMissingAlbums":false});
        }
        "album" => {
            let mut artist = body["artist"].clone();
            artist["rootFolderPath"] = json!(server.root_folder);
            artist["qualityProfileId"] = json!(server.quality_profile_id);
            artist["metadataProfileId"] = json!(server.metadata_profile_id);
            artist["monitored"] = json!(true);
            artist["monitorNewItems"] = json!("none");
            artist["addOptions"] = json!({"monitor":"none","albumsToMonitor":[input.external_id],"searchForMissingAlbums":false});
            body["artist"] = artist;
            body["addOptions"] = json!({"searchForNewAlbum":false});
        }
        _ => return Err(err(StatusCode::UNPROCESSABLE_ENTITY, "track_not_supported")),
    }
    let result = server
        .call("POST", endpoint(&input.kind), &[], Some(body))
        .await?;
    remote_id(&result)
}

/// Invoked once after a durable search-intent marker; a lost response is never blindly replayed.
pub async fn monitor_future(server: &Server, input: &RequestInput, id: &str) -> Result<(), Error> {
    if input.monitor_future && matches!(input.kind.as_str(), "series" | "artist") {
        let mut value = server
            .call("GET", &format!("{}/{id}", endpoint(&input.kind)), &[], None)
            .await?;
        value["monitored"] = json!(true);
        value["monitorNewItems"] = json!("all");
        server
            .call(
                "PUT",
                &format!("{}/{id}", endpoint(&input.kind)),
                &[],
                Some(value),
            )
            .await?;
    }
    Ok(())
}
/// Invoked once after a durable search-intent marker; a lost response is never blindly replayed.
pub async fn search_missing(server: &Server, input: &RequestInput, id: &str) -> Result<(), Error> {
    let number = id
        .parse::<i64>()
        .map_err(|_| err(StatusCode::BAD_GATEWAY, "invalid_remote_id"))?;
    match input.kind.as_str() {
        "film" => {
            server
                .call(
                    "POST",
                    "command",
                    &[],
                    Some(json!({"name":"MoviesSearch","movieIds":[number]})),
                )
                .await?;
        }
        "series" => {
            let mut series = server
                .call("GET", &format!("series/{number}"), &[], None)
                .await?;
            if let Some(seasons) = series["seasons"].as_array_mut() {
                for season in seasons {
                    let n = season["seasonNumber"].as_i64().unwrap_or(-1);
                    let selected = input.seasons.contains(&n)
                        || (input.seasons.is_empty() && input.episodes.is_empty());
                    if selected {
                        season["monitored"] = json!(true);
                    }
                }
            }
            series["monitored"] = json!(true);
            if input.monitor_future {
                series["monitorNewItems"] = json!("all");
            }
            server
                .call("PUT", &format!("series/{number}"), &[], Some(series))
                .await?;
            if !input.episodes.is_empty() {
                let episodes = server
                    .call("GET", "episode", &[("seriesId", id.into())], None)
                    .await?;
                let ids: Vec<_> = array(&episodes)?
                    .iter()
                    .filter(|e| {
                        input.episodes.iter().any(|s| {
                            e["seasonNumber"].as_i64() == Some(s.season)
                                && e["episodeNumber"].as_i64() == Some(s.episode)
                        })
                    })
                    .filter_map(|e| e["id"].as_i64())
                    .collect();
                if ids.len() != input.episodes.len() {
                    return Err(err(
                        StatusCode::UNPROCESSABLE_ENTITY,
                        "requested_episode_not_found",
                    ));
                }
                server
                    .call(
                        "PUT",
                        "episode/monitor",
                        &[],
                        Some(json!({"episodeIds":ids,"monitored":true})),
                    )
                    .await?;
                server
                    .call(
                        "POST",
                        "command",
                        &[],
                        Some(json!({"name":"EpisodeSearch","episodeIds":ids})),
                    )
                    .await?;
            }
            if input.seasons.is_empty() && input.episodes.is_empty() {
                server
                    .call(
                        "POST",
                        "command",
                        &[],
                        Some(json!({"name":"SeriesSearch","seriesId":number})),
                    )
                    .await?;
            }
            for season in &input.seasons {
                server
                    .call(
                        "POST",
                        "command",
                        &[],
                        Some(
                            json!({"name":"SeasonSearch","seriesId":number,"seasonNumber":season}),
                        ),
                    )
                    .await?;
            }
        }
        "album" => {
            server
                .call(
                    "PUT",
                    "album/monitor",
                    &[],
                    Some(json!({"albumIds":[number],"monitored":true})),
                )
                .await?;
            server
                .call(
                    "POST",
                    "command",
                    &[],
                    Some(json!({"name":"AlbumSearch","albumIds":[number]})),
                )
                .await?;
        }
        "artist" => {
            let mut artist = server
                .call("GET", &format!("artist/{number}"), &[], None)
                .await?;
            artist["monitored"] = json!(true);
            if input.monitor_future {
                artist["monitorNewItems"] = json!("all");
            }
            server
                .call("PUT", &format!("artist/{number}"), &[], Some(artist))
                .await?;
            if !input.album_ids.is_empty() {
                let albums = server
                    .call("GET", "album", &[("artistId", id.into())], None)
                    .await?;
                let ids: Vec<_> = array(&albums)?
                    .iter()
                    .filter(|a| {
                        a["foreignAlbumId"]
                            .as_str()
                            .is_some_and(|s| input.album_ids.iter().any(|id| id == s))
                    })
                    .filter_map(|a| a["id"].as_i64())
                    .collect();
                if ids.len() != input.album_ids.len() {
                    return Err(err(
                        StatusCode::UNPROCESSABLE_ENTITY,
                        "requested_album_not_found",
                    ));
                }
                server
                    .call(
                        "PUT",
                        "album/monitor",
                        &[],
                        Some(json!({"albumIds":ids,"monitored":true})),
                    )
                    .await?;
                server
                    .call(
                        "POST",
                        "command",
                        &[],
                        Some(json!({"name":"AlbumSearch","albumIds":ids})),
                    )
                    .await?;
            }
        }
        _ => return Err(err(StatusCode::UNPROCESSABLE_ENTITY, "track_not_supported")),
    }
    Ok(())
}
pub struct Progress {
    pub paths: Vec<String>,
    pub expected: usize,
    pub imported: bool,
}
fn paths_from(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|v| v["path"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}
pub async fn progress(server: &Server, input: &RequestInput, id: &str) -> Result<Progress, Error> {
    let number = id
        .parse::<i64>()
        .map_err(|_| err(StatusCode::BAD_GATEWAY, "invalid_remote_id"))?;
    match input.kind.as_str() {
        "film" => {
            let movie = server
                .call("GET", &format!("movie/{number}"), &[], None)
                .await?;
            let paths = movie["movieFile"]["path"]
                .as_str()
                .map(|s| vec![s.into()])
                .unwrap_or_default();
            Ok(Progress {
                paths,
                expected: 1,
                imported: movie["hasFile"] == true,
            })
        }
        "series" => {
            let episodes = server
                .call("GET", "episode", &[("seriesId", id.into())], None)
                .await?;
            let selected: Vec<_> = array(&episodes)?
                .iter()
                .filter(|e| {
                    input
                        .seasons
                        .contains(&e["seasonNumber"].as_i64().unwrap_or(-1))
                        || input.episodes.iter().any(|s| {
                            e["seasonNumber"].as_i64() == Some(s.season)
                                && e["episodeNumber"].as_i64() == Some(s.episode)
                        })
                        || (input.seasons.is_empty() && input.episodes.is_empty())
                })
                .collect();
            let files = server
                .call("GET", "episodefile", &[("seriesId", id.into())], None)
                .await?;
            let paths = array(&files)?
                .iter()
                .filter(|file| selected.iter().any(|e| e["episodeFileId"] == file["id"]))
                .filter_map(|v| v["path"].as_str().map(str::to_owned))
                .collect();
            let imported = !selected.is_empty()
                && selected.iter().all(|e| {
                    e["hasFile"] == true && e["episodeFileId"].as_i64().is_some_and(|id| id > 0)
                });
            let expected = if imported {
                selected
                    .iter()
                    .filter_map(|e| e["episodeFileId"].as_i64())
                    .collect::<std::collections::BTreeSet<_>>()
                    .len()
            } else {
                selected.len()
            };
            Ok(Progress {
                paths,
                expected: expected.max(1),
                imported,
            })
        }
        "album" => {
            let album = server
                .call("GET", &format!("album/{number}"), &[], None)
                .await?;
            let files = server
                .call("GET", "trackfile", &[("albumId", id.into())], None)
                .await?;
            let expected = album["statistics"]["trackCount"].as_u64().unwrap_or(0) as usize;
            let paths = paths_from(&files);
            Ok(Progress {
                imported: expected > 0 && paths.len() >= expected,
                expected: expected.max(1),
                paths,
            })
        }
        "artist" => {
            let albums = server
                .call("GET", "album", &[("artistId", id.into())], None)
                .await?;
            let selected: Vec<_> = array(&albums)?
                .iter()
                .filter(|a| {
                    a["foreignAlbumId"]
                        .as_str()
                        .is_some_and(|s| input.album_ids.iter().any(|id| id == s))
                })
                .collect();
            let mut paths = vec![];
            let mut expected = 0;
            for album in selected {
                let album_id = album["id"]
                    .as_i64()
                    .ok_or_else(|| err(StatusCode::BAD_GATEWAY, "invalid_remote_id"))?;
                let files = server
                    .call(
                        "GET",
                        "trackfile",
                        &[("albumId", album_id.to_string())],
                        None,
                    )
                    .await?;
                paths.extend(paths_from(&files));
                expected += album["statistics"]["trackCount"].as_u64().unwrap_or(0) as usize;
            }
            Ok(Progress {
                imported: expected > 0 && paths.len() >= expected,
                expected: expected.max(1),
                paths,
            })
        }
        _ => Err(err(StatusCode::UNPROCESSABLE_ENTITY, "track_not_supported")),
    }
}
