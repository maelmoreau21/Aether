use super::*;

/// Merge only absent automatic fields. Manual corrections remain a separate
/// higher-precedence document and are never modified by this path.
pub async fn apply_missing_metadata(
    tx: &mut Transaction<'_, Any>,
    asset: &str,
    extension: &str,
    fingerprint: &str,
    result: &Value,
    now: i64,
) -> Result<(), sqlx::Error> {
    let rows: Vec<(String, String, String, String)> = query_as(
        "SELECT w.id,w.editorial_json,COALESCE(w.manual_json,'{}'),a.library_id \
            FROM inventory_works w \
            JOIN inventory_editions e ON e.work_id=w.id \
            JOIN inventory_versions v ON v.edition_id=e.id \
            JOIN inventory_sources s ON s.version_id=v.id \
            JOIN inventory_assets a ON a.source_id=s.id \
            WHERE a.id=?",
    )
    .bind(asset)
    .fetch_all(&mut **tx)
    .await?;
    for (work, raw, manual_raw, library) in rows {
        let mut metadata: Value =
            serde_json::from_str(&raw).unwrap_or_else(|_| serde_json::json!({}));
        let Some(fields) = metadata.as_object_mut() else {
            continue;
        };
        let manual: Value = serde_json::from_str(&manual_raw).unwrap_or(Value::Null);
        if manual.get("locked").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        let locked_fields: Vec<String> = manual
            .get("locked_fields")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .map(|s| s.to_ascii_lowercase())
                    .collect()
            })
            .unwrap_or_default();
        let is_field_locked = |dest: &str| -> bool {
            let group = match dest {
                "title" | "original_title" | "sort_title" => "title",
                "plot" | "outline" | "tagline" => "plot",
                "genres" => "genres",
                "mpaa" => "mpaa",
                "directors" | "writers" | "actors" | "crewCredits" => "people",
                "countries" => "countries",
                "studios" => "studios",
                "tags" => "tags",
                other => other,
            };
            locked_fields.iter().any(|f| f == dest || f == group)
        };
        let priority: i64 = query_scalar(
            "SELECT COALESCE((SELECT priority FROM automation_policies WHERE extension_id=? \
                AND library_id=?),(SELECT priority FROM automation_policies WHERE extension_id=? \
                AND library_id=(SELECT parent_id FROM library_folders WHERE library_id=? \
                AND active=1 LIMIT 1)),100)",
        )
        .bind(extension)
        .bind(&library)
        .bind(extension)
        .bind(&library)
        .fetch_one(&mut **tx)
        .await?;
        let (options, library_kind): (String, String) =
            query_as("SELECT options_json, kind FROM libraries WHERE id=?")
                .bind(&library)
                .fetch_one(&mut **tx)
                .await?;
        let options: crate::library_options::LibraryOptions = serde_json::from_str(&options)
            .map_err(|_| sqlx::Error::Protocol("invalid library options".into()))?;
        let Some(priority) =
            options.metadata_provider_priority_for_kind(&library_kind, extension, priority)
        else {
            continue;
        };
        let provider_id = result["providerId"]
            .as_str()
            .unwrap_or("")
            .chars()
            .take(256)
            .collect::<String>();
        let proposal = result
            .get("metadata")
            .filter(|v| v.is_object())
            .unwrap_or(result);
        let mut changed = false;
        for (destination, sources) in [
            ("plot", &["plot", "synopsis", "summary", "overview"][..]),
            ("outline", &["outline", "tagline"][..]),
            ("tagline", &["tagline"][..]),
            ("genres", &["genres"][..]),
            ("directors", &["directors"][..]),
            ("writers", &["writers"][..]),
            ("actors", &["actors"][..]),
            ("ratings", &["ratings"][..]),
            ("runtime", &["runtime", "runtimeMinutes"][..]),
            ("mpaa", &["mpaa"][..]),
            ("premiered", &["premiered", "releaseDate"][..]),
            ("year", &["year"][..]),
            ("collection", &["collection"][..]),
        ] {
            if is_field_locked(destination)
                || manual
                    .as_object()
                    .is_some_and(|m| m.get(destination).is_some_and(|v| !v.is_null()))
            {
                continue;
            }
            let value = sources
                .iter()
                .filter_map(|key| proposal.get(*key))
                .find(|v| match destination {
                    "genres" | "directors" | "writers" => v.as_array().is_some_and(|items| {
                        !items.is_empty()
                            && items.len() <= 64
                            && items.iter().all(|item| {
                                item.as_str()
                                    .is_some_and(|s| !s.is_empty() && s.len() <= 128)
                            })
                    }),
                    "collection" => {
                        v["id"].as_str().is_some_and(|id| {
                            !id.is_empty()
                                && id.len() <= 100
                                && id.bytes().all(|c| {
                                    c.is_ascii_alphanumeric() || matches!(c, b':' | b'-' | b'_')
                                })
                        }) && v["name"].as_str().is_some_and(|name| {
                            !name.trim().is_empty()
                                && name.len() <= 300
                                && !name.chars().any(char::is_control)
                        })
                    }
                    "actors" => v.as_array().is_some_and(|items| {
                        !items.is_empty()
                            && items.len() <= 64
                            && items.iter().all(|item| item.is_object())
                    }),
                    "ratings" => v.as_array().is_some_and(|items| {
                        !items.is_empty()
                            && items.len() <= 16
                            && items.iter().all(|item| item.is_object())
                    }),
                    "runtime" | "year" => v.as_u64().is_some() || v.as_i64().is_some(),
                    _ => v
                        .as_str()
                        .is_some_and(|s| !s.trim().is_empty() && s.len() <= 16384),
                });
            let Some(value) = value else { continue };
            let current = fields.get(destination);
            let empty = current.is_none_or(|v| {
                v.is_null() || v.as_str() == Some("") || v.as_array().is_some_and(Vec::is_empty)
            });
            let previous:Option<(String,i64,String)>=query_as("SELECT extension_id,priority,value_json FROM automatic_metadata_provenance WHERE work_id=? AND field_name=?")
                .bind(&work).bind(destination).fetch_optional(&mut **tx).await?;
            let replace = empty
                || previous
                    .as_ref()
                    .is_some_and(|(owner, old_priority, old_value)| {
                        serde_json::from_str::<Value>(old_value)
                            .is_ok_and(|ov| current == Some(&ov))
                            && (owner == extension
                                || priority
                                    > crate::library_options::LibraryOptions::provider_priority(
                                        &options.metadata_providers,
                                        owner,
                                        *old_priority,
                                    )
                                    .unwrap_or(-1))
                    });
            if !replace {
                continue;
            }
            fields.insert(destination.into(), value.clone());
            query("INSERT INTO automatic_metadata_provenance(work_id,field_name,extension_id,provider_id,priority,value_json,source_fingerprint,updated_at) VALUES(?,?,?,?,?,?,?,?) ON CONFLICT(work_id,field_name) DO UPDATE \
                    SET extension_id=excluded.extension_id,provider_id=excluded.provider_id,priority=excluded.priority,value_json=excluded.value_json,source_fingerprint=excluded.source_fingerprint,updated_at=excluded.updated_at")
                .bind(&work).bind(destination).bind(extension).bind(&provider_id).bind(priority).bind(value.to_string()).bind(fingerprint).bind(now).execute(&mut **tx).await?;
            changed = true;
        }
        if let Some(ext_ids) = proposal
            .get("externalIds")
            .or_else(|| proposal.get("external_ids"))
            .and_then(Value::as_object)
        {
            let current_ids_raw: Option<String> =
                query_scalar("SELECT external_ids_json FROM inventory_works WHERE id=?")
                    .bind(&work)
                    .fetch_optional(&mut **tx)
                    .await?;
            let mut current_ids: std::collections::BTreeMap<String, String> = current_ids_raw
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or_default();
            let mut ids_changed = false;
            for (k, v) in ext_ids {
                if let Some(str_val) = v.as_str().filter(|s| !s.is_empty())
                    && !current_ids.contains_key(k)
                {
                    current_ids.insert(k.clone(), str_val.to_string());
                    ids_changed = true;
                }
            }
            if ids_changed {
                query("UPDATE inventory_works SET external_ids_json=? WHERE id=?")
                    .bind(serde_json::to_string(&current_ids).unwrap_or_default())
                    .bind(&work)
                    .execute(&mut **tx)
                    .await?;
            }
        }
        // Provider translations never replace each other's or manual rows: they are
        // stored per (locale, provider) and ranked when read.
        if crate::translations::store_proposal_translations(tx, extension, &work, proposal).await?
            > 0
        {
            changed = true;
        }
        let kind: Option<String> = query_scalar("SELECT kind FROM inventory_works WHERE id=?")
            .bind(&work)
            .fetch_optional(&mut **tx)
            .await?;
        if kind.as_deref() == Some("series")
            && let Some(seasons) = proposal.get("seasons").and_then(Value::as_array)
        {
            for season in seasons {
                let s_num = season
                    .get("seasonNumber")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                if let Some(s_title) = season
                    .get("title")
                    .and_then(Value::as_str)
                    .filter(|s| !s.trim().is_empty())
                {
                    let _ = query(
                        "UPDATE inventory_seasons SET title=? WHERE series_id=? AND season_number=?",
                    )
                    .bind(s_title)
                    .bind(&work)
                    .bind(s_num as i64)
                    .execute(&mut **tx)
                    .await;
                }
                if let Some(episodes) = season.get("episodes").and_then(Value::as_array) {
                    for ep in episodes {
                        let ep_num = ep.get("episodeNumber").and_then(Value::as_u64).unwrap_or(0);
                        let ep_work: Option<String> = query_scalar(
                            "SELECT ep.work_id FROM inventory_episodes ep JOIN inventory_seasons s ON s.id=ep.season_id WHERE s.series_id=? AND s.season_number=? AND ep.episode_number=?",
                        )
                        .bind(&work)
                        .bind(s_num as i64)
                        .bind(ep_num as i64)
                        .fetch_optional(&mut **tx)
                        .await
                        .unwrap_or(None);

                        if let Some(ep_wid) = ep_work {
                            let mut ep_meta = serde_json::Map::new();
                            if let Some(t) = ep
                                .get("title")
                                .and_then(Value::as_str)
                                .filter(|s| !s.trim().is_empty())
                            {
                                ep_meta.insert("title".into(), json!(t));
                                let _ = query(
                                    "UPDATE inventory_works SET title=? WHERE id=? AND (title LIKE 'S%E%' OR title LIKE 'Episode%' OR title='')",
                                )
                                .bind(t)
                                .bind(&ep_wid)
                                .execute(&mut **tx)
                                .await;
                            }
                            if let Some(p) = ep
                                .get("overview")
                                .and_then(Value::as_str)
                                .filter(|s| !s.trim().is_empty())
                            {
                                ep_meta.insert("plot".into(), json!(p));
                                ep_meta.insert("outline".into(), json!(p));
                            }
                            if let Some(d) = ep.get("airDate").and_then(Value::as_str) {
                                ep_meta.insert("premiered".into(), json!(d));
                            }
                            if let Some(r) = ep.get("runtimeMinutes").and_then(Value::as_u64) {
                                ep_meta.insert("runtime".into(), json!(r));
                            }
                            if let Some(st) = ep.get("stillUrl").and_then(Value::as_str) {
                                ep_meta.insert("still_url".into(), json!(st));
                                ep_meta.insert("thumb_url".into(), json!(st));
                            }
                            if let Some(rt) = ep.get("rating").and_then(Value::as_f64) {
                                ep_meta.insert("rating".into(), json!(rt));
                            }
                            let ep_json = serde_json::to_string(&ep_meta).unwrap_or_default();
                            let _ = query("UPDATE inventory_works SET editorial_json=? WHERE id=?")
                                .bind(&ep_json)
                                .bind(&ep_wid)
                                .execute(&mut **tx)
                                .await;
                        }
                    }
                }
            }
        }
        if changed {
            query("UPDATE inventory_works SET editorial_json=? WHERE id=?")
                .bind(metadata.to_string())
                .bind(&work)
                .execute(&mut **tx)
                .await?;
        }

        let (has_poster, has_backdrop): (i64, i64) = query_as(
            "SELECT CAST((SELECT COUNT(*) FROM work_image_entries WHERE work_id=? AND kind='poster') + (SELECT COUNT(*) FROM work_posters WHERE work_id=?) AS INTEGER), CAST((SELECT COUNT(*) FROM work_image_entries WHERE work_id=? AND kind='backdrop') AS INTEGER)"
        )
        .bind(&work)
        .bind(&work)
        .bind(&work)
        .fetch_one(&mut **tx)
        .await
        .unwrap_or((0, 0));

        let images_allowed = crate::library_options::LibraryOptions::provider_priority(
            &options.image_providers,
            extension,
            100,
        )
        .is_some();
        let poster_url = if has_poster == 0 && options.fetch_poster && images_allowed {
            proposal
                .get("posterUrl")
                .or_else(|| proposal.get("poster_url"))
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| {
                    proposal
                        .get("posters")
                        .and_then(Value::as_array)
                        .and_then(|arr| arr.first())
                        .and_then(|p| p.get("url").and_then(Value::as_str))
                        .map(str::to_string)
                })
                .or_else(|| {
                    proposal
                        .get("images")
                        .and_then(Value::as_array)
                        .and_then(|arr| {
                            arr.iter().find(|img| {
                                img.get("imageType").and_then(Value::as_str) == Some("poster")
                            })
                        })
                        .and_then(|p| p.get("url").and_then(Value::as_str))
                        .map(str::to_string)
                })
        } else {
            None
        };

        let backdrop_url = if has_backdrop == 0 && options.fetch_backdrop && images_allowed {
            proposal
                .get("backdropUrl")
                .or_else(|| proposal.get("backdrop_url"))
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| {
                    proposal
                        .get("backdrops")
                        .and_then(Value::as_array)
                        .and_then(|arr| arr.first())
                        .and_then(|p| p.get("url").and_then(Value::as_str))
                        .map(str::to_string)
                })
                .or_else(|| {
                    proposal
                        .get("images")
                        .and_then(Value::as_array)
                        .and_then(|arr| {
                            arr.iter().find(|img| {
                                img.get("imageType").and_then(Value::as_str) == Some("backdrop")
                            })
                        })
                        .and_then(|p| p.get("url").and_then(Value::as_str))
                        .map(str::to_string)
                })
        } else {
            None
        };

        // The background image filler completes the other kinds (logo, banner…) or the
        // multi-image pool of this newly identified work on its next pass.
        crate::extensions::request_artwork_check();
        if (poster_url.is_some() || backdrop_url.is_some())
            && let Some(store) = crate::identity::active_store()
        {
            let ext_id = extension.to_string();
            let work_id = work.clone();
            tokio::spawn(async move {
                if let Some(url) = poster_url {
                    let _ = crate::extensions::adapters::import_image_direct(
                        &store,
                        &ext_id,
                        crate::extensions::adapters::ImportImage {
                            work_id: work_id.clone(),
                            url,
                            kind: "poster".into(),
                            language: None,
                            append: false,
                            apply_collection_policy: false,
                        },
                    )
                    .await;
                }
                if let Some(url) = backdrop_url {
                    let _ = crate::extensions::adapters::import_image_direct(
                        &store,
                        &ext_id,
                        crate::extensions::adapters::ImportImage {
                            work_id: work_id.clone(),
                            url,
                            kind: "backdrop".into(),
                            language: None,
                            append: false,
                            apply_collection_policy: false,
                        },
                    )
                    .await;
                }
            });
        }
    }
    Ok(())
}

/// Fence, persist output and complete the leased job atomically.
pub async fn apply_missing_music_metadata(
    tx: &mut Transaction<'_, Any>,
    track_id: &str,
    extension: &str,
    fingerprint: &str,
    result: &Value,
    now: i64,
) -> Result<(), sqlx::Error> {
    let row: Option<MusicMetadataRow> = query_as(
        "SELECT library_id, title, artist, album, album_artist, duration, lyrics, cover_path FROM music_tracks WHERE id=?"
    )
    .bind(track_id)
    .fetch_optional(&mut **tx)
    .await?;
    let Some((library, _title, artist, album, _album_artist, _duration, lyrics, cover_path)) = row
    else {
        return Ok(());
    };
    let priority: i64 = query_scalar(
        "SELECT COALESCE((SELECT priority FROM automation_policies WHERE extension_id=? AND library_id=?),(SELECT priority FROM automation_policies WHERE extension_id=? AND library_id=(SELECT parent_id FROM library_folders WHERE library_id=? AND active=1 LIMIT 1)),100)"
    )
    .bind(extension).bind(&library).bind(extension).bind(&library).fetch_one(&mut **tx).await?;

    let options: String = query_scalar("SELECT options_json FROM libraries WHERE id=?")
        .bind(&library)
        .fetch_one(&mut **tx)
        .await?;
    let options: crate::library_options::LibraryOptions = serde_json::from_str(&options)
        .map_err(|_| sqlx::Error::Protocol("invalid library options".into()))?;
    let Some(priority) = crate::library_options::LibraryOptions::provider_priority(
        &options.metadata_providers,
        extension,
        priority,
    ) else {
        return Ok(());
    };
    let provider_id = result["providerId"]
        .as_str()
        .or_else(|| result["id"].as_str())
        .map(str::to_owned)
        .or_else(|| result["id"].as_u64().map(|id| id.to_string()))
        .unwrap_or_default()
        .chars()
        .take(256)
        .collect::<String>();

    // 1. Lyrics enrichment: preserve manual lyrics, allow higher priority
    let proposal_lyrics = ["synced", "plain", "lyrics"]
        .into_iter()
        .filter_map(|field| result[field].as_str())
        .find(|text| !text.trim().is_empty() && text.len() <= 32768);

    if let Some(new_lyrics) = proposal_lyrics {
        let previous: Option<(i64, String)> = query_as(
            "SELECT priority, value_json FROM automatic_metadata_provenance WHERE work_id=? AND field_name='lyrics'"
        ).bind(track_id).fetch_optional(&mut **tx).await?;
        let previous_matches = previous.as_ref().is_some_and(|(_, value)| {
            serde_json::from_str::<String>(value).ok().as_deref() == Some(lyrics.as_str())
        });
        // A rescan or manual edit may have replaced the cached provider value.
        // Provenance alone must never authorize overwriting those local lyrics.
        if lyrics.is_empty() || previous_matches {
            let previous_prio = previous
                .as_ref()
                .filter(|_| previous_matches)
                .map(|(priority, _)| *priority);
            if previous_prio.is_none_or(|p| priority >= p) {
                query("UPDATE music_tracks SET lyrics=? WHERE id=?")
                    .bind(new_lyrics)
                    .bind(track_id)
                    .execute(&mut **tx)
                    .await?;
                query(
                    "INSERT INTO automatic_metadata_provenance(work_id,field_name,extension_id,provider_id,priority,value_json,source_fingerprint,updated_at) VALUES(?,?,?,?,?,?,?,?) ON CONFLICT(work_id,field_name) DO UPDATE \
                            SET extension_id=excluded.extension_id,provider_id=excluded.provider_id,priority=excluded.priority,value_json=excluded.value_json,source_fingerprint=excluded.source_fingerprint,updated_at=excluded.updated_at"
                )
                .bind(track_id).bind("lyrics").bind(extension).bind(&provider_id).bind(priority).bind(serde_json::json!(new_lyrics).to_string()).bind(fingerprint).bind(now).execute(&mut **tx).await?;
            }
        }
    }

    // 2. Artist & Album enrichment if missing
    let proposal_artist = result["artist"]
        .as_str()
        .or_else(|| result["artistName"].as_str());
    if let Some(new_artist) = proposal_artist.filter(|s| !s.trim().is_empty())
        && (artist.is_empty() || artist == "Unknown Artist")
    {
        query("UPDATE music_tracks SET artist=? WHERE id=?")
            .bind(new_artist)
            .bind(track_id)
            .execute(&mut **tx)
            .await?;
        query(
            "INSERT INTO automatic_metadata_provenance(work_id,field_name,extension_id,provider_id,priority,value_json,source_fingerprint,updated_at) VALUES(?,?,?,?,?,?,?,?) ON CONFLICT(work_id,field_name) DO UPDATE \
                    SET extension_id=excluded.extension_id,provider_id=excluded.provider_id,priority=excluded.priority,value_json=excluded.value_json,source_fingerprint=excluded.source_fingerprint,updated_at=excluded.updated_at"
        )
        .bind(track_id).bind("artist").bind(extension).bind(&provider_id).bind(priority).bind(serde_json::json!(new_artist).to_string()).bind(fingerprint).bind(now).execute(&mut **tx).await?;
    }

    let proposal_album = result["album"]
        .as_str()
        .or_else(|| result["albumName"].as_str());
    if let Some(new_album) = proposal_album.filter(|s| !s.trim().is_empty())
        && (album.is_empty() || album == "Unknown Album")
    {
        query("UPDATE music_tracks SET album=? WHERE id=?")
            .bind(new_album)
            .bind(track_id)
            .execute(&mut **tx)
            .await?;
        query(
            "INSERT INTO automatic_metadata_provenance(work_id,field_name,extension_id,provider_id,priority,value_json,source_fingerprint,updated_at) VALUES(?,?,?,?,?,?,?,?) ON CONFLICT(work_id,field_name) DO UPDATE \
                    SET extension_id=excluded.extension_id,provider_id=excluded.provider_id,priority=excluded.priority,value_json=excluded.value_json,source_fingerprint=excluded.source_fingerprint,updated_at=excluded.updated_at"
        )
        .bind(track_id).bind("album").bind(extension).bind(&provider_id).bind(priority).bind(serde_json::json!(new_album).to_string()).bind(fingerprint).bind(now).execute(&mut **tx).await?;
    }

    // 3. Cover art enrichment: preserve local album cover
    let proposal_cover = result["coverUrl"]
        .as_str()
        .or_else(|| result["cover_url"].as_str())
        .or_else(|| result["poster_url"].as_str())
        .or_else(|| result["coverPath"].as_str());
    if let Some(cover_url) = proposal_cover.filter(|s| !s.trim().is_empty())
        && cover_path.is_none()
    {
        query("UPDATE music_tracks SET cover_path=? WHERE id=?")
            .bind(cover_url)
            .bind(track_id)
            .execute(&mut **tx)
            .await?;
        query(
            "INSERT INTO automatic_metadata_provenance(work_id,field_name,extension_id,provider_id,priority,value_json,source_fingerprint,updated_at) VALUES(?,?,?,?,?,?,?,?) ON CONFLICT(work_id,field_name) DO UPDATE \
                    SET extension_id=excluded.extension_id,provider_id=excluded.provider_id,priority=excluded.priority,value_json=excluded.value_json,source_fingerprint=excluded.source_fingerprint,updated_at=excluded.updated_at"
        )
        .bind(track_id).bind("cover").bind(extension).bind(&provider_id).bind(priority).bind(serde_json::json!(cover_url).to_string()).bind(fingerprint).bind(now).execute(&mut **tx).await?;
    }

    Ok(())
}

pub(super) fn metadata_has_missing_fields(editorial: &str, manual: &str) -> bool {
    let editorial = serde_json::from_str::<Value>(editorial).unwrap_or(Value::Null);
    let manual = serde_json::from_str::<Value>(manual).unwrap_or(Value::Null);
    if manual.get("locked").and_then(Value::as_bool) == Some(true) {
        return false;
    }
    ["plot", "outline", "genres"].iter().any(|key| {
        !manual
            .as_object()
            .is_some_and(|fields| fields.contains_key(*key))
            && editorial.get(*key).is_none_or(|value| {
                value.is_null()
                    || value.as_str() == Some("")
                    || value.as_array().is_some_and(Vec::is_empty)
            })
    })
}
