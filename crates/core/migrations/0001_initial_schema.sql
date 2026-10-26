-- Aether initial unified schema (SQLite)
-- Replaces the historical 52 beta migrations with a clean, coherent baseline.
-- Assets and images are content-addressed and deduplicated via assets(sha256).

CREATE TABLE assets (
    sha256 TEXT PRIMARY KEY CHECK(length(sha256)=64),
    size INTEGER NOT NULL CHECK(size>0),
    mime TEXT NOT NULL CHECK(length(mime) BETWEEN 3 AND 100),
    created_at INTEGER NOT NULL,
    orphaned_at INTEGER
) STRICT;
CREATE INDEX assets_orphaned ON assets(orphaned_at) WHERE orphaned_at IS NOT NULL;

CREATE TABLE users (
    id TEXT PRIMARY KEY NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('administrator', 'user')),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    disabled_at INTEGER
, display_name TEXT, sso_username TEXT) STRICT;

CREATE TABLE local_credentials (
    user_id TEXT PRIMARY KEY NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    login TEXT NOT NULL UNIQUE CHECK(length(login) BETWEEN 3 AND 64 AND login NOT GLOB '*[^a-z0-9._-]*'),
    password_hash TEXT NOT NULL,
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;

CREATE TABLE bootstrap_state (
    singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
    completed_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;

CREATE TABLE sessions (
    token_hash TEXT PRIMARY KEY NOT NULL,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL,
    last_seen_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
, device_label TEXT NOT NULL DEFAULT 'Navigateur') STRICT;

CREATE INDEX sessions_user ON sessions(user_id);

CREATE TABLE library_permissions (
    library_id TEXT NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    PRIMARY KEY(library_id, user_id)
) STRICT;

CREATE INDEX library_permissions_user ON library_permissions(user_id, library_id);

CREATE TABLE library_initialization (
    singleton INTEGER PRIMARY KEY CHECK(singleton = 1)
) STRICT;

CREATE TABLE inventory_works (id TEXT PRIMARY KEY, library_id TEXT NOT NULL REFERENCES libraries(id), identity_key TEXT NOT NULL, title TEXT NOT NULL, year INTEGER, external_ids_json TEXT NOT NULL, editorial_json TEXT NOT NULL DEFAULT '{}', title_sort TEXT NOT NULL DEFAULT '', kind TEXT NOT NULL DEFAULT 'film', added_at INTEGER, manual_json TEXT, minimum_age INTEGER CHECK(minimum_age BETWEEN 0 AND 21), display_title_sort TEXT NOT NULL DEFAULT '', effective_year INTEGER, max_height INTEGER NOT NULL DEFAULT 0, codecs_csv TEXT NOT NULL DEFAULT ',', languages_csv TEXT NOT NULL DEFAULT ',', genres_csv TEXT NOT NULL DEFAULT '|', has_available_asset INTEGER NOT NULL DEFAULT 0 CHECK(has_available_asset IN (0, 1)), season_count_cached INTEGER NOT NULL DEFAULT 0, episode_count_cached INTEGER NOT NULL DEFAULT 0, UNIQUE(library_id,identity_key)) STRICT;

CREATE TABLE inventory_editions (id TEXT PRIMARY KEY, work_id TEXT NOT NULL REFERENCES inventory_works(id)) STRICT;

CREATE TABLE inventory_versions (id TEXT PRIMARY KEY, edition_id TEXT NOT NULL REFERENCES inventory_editions(id)) STRICT;

CREATE TABLE inventory_sources (root_snapshot TEXT NOT NULL, id TEXT PRIMARY KEY, version_id TEXT NOT NULL REFERENCES inventory_versions(id), library_id TEXT NOT NULL REFERENCES libraries(id)) STRICT;

CREATE TABLE inventory_assets (id TEXT PRIMARY KEY, source_id TEXT NOT NULL REFERENCES inventory_sources(id), library_id TEXT NOT NULL REFERENCES libraries(id), relative_path TEXT NOT NULL, size INTEGER NOT NULL, modified_ns TEXT NOT NULL, mime TEXT NOT NULL, probe_json TEXT NOT NULL, available INTEGER NOT NULL CHECK(available IN (0,1)), UNIQUE(library_id,relative_path)) STRICT;

CREATE TABLE inventory_tracks (id TEXT PRIMARY KEY, asset_id TEXT NOT NULL REFERENCES inventory_assets(id), stream_index INTEGER NOT NULL, metadata_json TEXT NOT NULL, UNIQUE(asset_id,stream_index)) STRICT;

CREATE INDEX inventory_sources_library ON inventory_sources(library_id);

CREATE TABLE scan_jobs (library_id TEXT PRIMARY KEY REFERENCES libraries(id), status TEXT NOT NULL, discovered INTEGER NOT NULL DEFAULT 0, probed INTEGER NOT NULL DEFAULT 0, reused INTEGER NOT NULL DEFAULT 0, errors_json TEXT NOT NULL DEFAULT '[]', errors_count INTEGER NOT NULL DEFAULT 0, warnings_count INTEGER NOT NULL DEFAULT 0, refresh_mode TEXT NOT NULL DEFAULT 'incremental'
    CHECK(refresh_mode IN ('incremental','new_only','missing_metadata','full'))) STRICT;

CREATE TABLE inventory_editorial (
    asset_id TEXT PRIMARY KEY REFERENCES inventory_assets(id),
    fallback_json TEXT NOT NULL,
    nfo_json TEXT,
    nfo_path TEXT,
    nfo_priority INTEGER
) STRICT;

CREATE TABLE inventory_images (
    id TEXT PRIMARY KEY,
    work_id TEXT NOT NULL REFERENCES inventory_works(id),
    library_id TEXT NOT NULL REFERENCES libraries(id),
    root_snapshot TEXT NOT NULL,
    relative_path TEXT NOT NULL,
    size INTEGER NOT NULL,
    modified_ns TEXT NOT NULL,
    mime TEXT NOT NULL,
    priority INTEGER NOT NULL,
    available INTEGER NOT NULL DEFAULT 1 CHECK(available IN (0,1)), role TEXT NOT NULL DEFAULT 'poster',
    UNIQUE(work_id,root_snapshot,relative_path)
);

CREATE INDEX inventory_images_work ON inventory_images(work_id,available,priority,relative_path);

CREATE INDEX inventory_works_library_sort ON inventory_works(library_id, title_sort, id);

CREATE INDEX inventory_works_title_sort ON inventory_works(title_sort, id);

CREATE INDEX inventory_editions_work ON inventory_editions(work_id);

CREATE INDEX inventory_versions_edition ON inventory_versions(edition_id);

CREATE INDEX inventory_sources_version ON inventory_sources(version_id);

CREATE INDEX inventory_assets_source_available ON inventory_assets(source_id, available);

CREATE TABLE inventory_works_unicode_migration (
    completed_at TEXT NOT NULL
);

CREATE TABLE scan_staging_files (
    scan_id TEXT NOT NULL,
    library_id TEXT NOT NULL,
    relative_path TEXT NOT NULL,
    size INTEGER NOT NULL,
    modified_ns TEXT NOT NULL,
    mime TEXT NOT NULL,
    title TEXT NOT NULL,
    year INTEGER,
    identity_key TEXT NOT NULL,
    external_ids_json TEXT NOT NULL,
    probe_json TEXT,
    reused INTEGER NOT NULL,
    poster_path TEXT,
    poster_size INTEGER,
    poster_modified_ns TEXT,
    poster_mime TEXT,
    poster_priority INTEGER,
    editorial_outcome TEXT NOT NULL,
    editorial_json TEXT,
    editorial_path TEXT,
    editorial_priority INTEGER, series_identity_key TEXT, series_title TEXT, series_year INTEGER, series_external_ids_json TEXT, series_editorial_json TEXT, series_poster_path TEXT, series_poster_size INTEGER, series_poster_modified_ns TEXT, series_poster_mime TEXT, series_poster_priority INTEGER, season_number INTEGER, episode_number INTEGER, subtitles_json TEXT NOT NULL DEFAULT '[]', audios_json TEXT NOT NULL DEFAULT '[]', artwork_json TEXT NOT NULL DEFAULT '{}',
    PRIMARY KEY (scan_id, relative_path)
);

CREATE TABLE scan_staging_issues (
    scan_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    relative_path TEXT NOT NULL,
    message TEXT NOT NULL
);

CREATE INDEX scan_staging_files_scan_reused ON scan_staging_files(scan_id, reused);

CREATE INDEX scan_staging_files_scan_library ON scan_staging_files(scan_id, library_id);

CREATE INDEX inventory_images_library_available ON inventory_images(library_id, available);

CREATE TABLE inventory_seasons (
    id TEXT PRIMARY KEY,
    series_id TEXT NOT NULL REFERENCES inventory_works(id) ON DELETE CASCADE,
    season_number INTEGER NOT NULL,
    title TEXT NOT NULL DEFAULT '',
    UNIQUE(series_id, season_number)
) STRICT;

CREATE TABLE inventory_episodes (
    id TEXT PRIMARY KEY,
    season_id TEXT NOT NULL REFERENCES inventory_seasons(id) ON DELETE CASCADE,
    episode_number INTEGER NOT NULL,
    work_id TEXT NOT NULL REFERENCES inventory_works(id) ON DELETE CASCADE,
    UNIQUE(season_id, episode_number),
    UNIQUE(work_id)
) STRICT;

CREATE INDEX inventory_works_library_kind ON inventory_works(library_id, kind);

CREATE INDEX inventory_seasons_series ON inventory_seasons(series_id, season_number);

CREATE INDEX inventory_episodes_season ON inventory_episodes(season_id, episode_number);

CREATE INDEX inventory_episodes_work ON inventory_episodes(work_id);

CREATE TABLE "media_nodes" (
    id TEXT PRIMARY KEY NOT NULL CHECK(length(id) BETWEEN 1 AND 128),
    name TEXT NOT NULL DEFAULT 'Local Media Node' CHECK(length(name) BETWEEN 1 AND 100),
    kind TEXT NOT NULL DEFAULT 'local' CHECK(kind IN ('local', 'remote')),
    base_url TEXT,
    public_url TEXT,
    status TEXT NOT NULL DEFAULT 'connected' CHECK(status IN ('connected', 'disconnected', 'revoked')),
    key_id TEXT NOT NULL DEFAULT 'local',
    shared_secret TEXT NOT NULL DEFAULT '',
    roots_json TEXT NOT NULL DEFAULT '[]',
    paired_at INTEGER NOT NULL DEFAULT (unixepoch()),
    last_seen_at INTEGER,
    revoked_at INTEGER
, version TEXT, connection_mode TEXT NOT NULL DEFAULT 'outbound') STRICT;

CREATE TABLE oidc_identities (
    issuer TEXT NOT NULL,
    subject TEXT NOT NULL CHECK(length(subject) BETWEEN 1 AND 255),
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    PRIMARY KEY(issuer, subject),
    UNIQUE(issuer, user_id)
) STRICT;

CREATE TABLE oidc_flows (
    state_hash TEXT PRIMARY KEY NOT NULL,
    browser_hash TEXT NOT NULL,
    nonce TEXT NOT NULL,
    verifier TEXT NOT NULL,
    link_session_hash TEXT REFERENCES sessions(token_hash) ON DELETE CASCADE,
    expires_at INTEGER NOT NULL
) STRICT;

CREATE INDEX oidc_flows_expiry ON oidc_flows(expires_at);

CREATE TABLE inventory_audio (
    asset_id TEXT PRIMARY KEY REFERENCES inventory_assets(id) ON DELETE CASCADE,
    audios_json TEXT NOT NULL
);

CREATE TABLE inventory_subtitles (
    asset_id TEXT PRIMARY KEY REFERENCES inventory_assets(id) ON DELETE CASCADE,
    subtitles_json TEXT NOT NULL
);

CREATE TABLE playback_progress (
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    work_id TEXT NOT NULL REFERENCES inventory_works(id) ON DELETE CASCADE,
    position_seconds DOUBLE PRECISION NOT NULL CHECK(position_seconds >= 0),
    duration_seconds DOUBLE PRECISION NOT NULL CHECK(duration_seconds > 0),
    watched INTEGER NOT NULL CHECK(watched IN (0,1)),
    revision BIGINT NOT NULL CHECK(revision > 0),
    updated_at BIGINT NOT NULL,
    PRIMARY KEY(user_id,work_id)
);

CREATE INDEX playback_progress_recent ON playback_progress(user_id,updated_at DESC);

CREATE INDEX inventory_works_added_at ON inventory_works(added_at, id);

CREATE TRIGGER inventory_works_added_at_insert AFTER INSERT ON inventory_works
WHEN NEW.added_at IS NULL BEGIN
  UPDATE inventory_works SET added_at=unixepoch() WHERE id=NEW.id;
END;

CREATE TABLE node_enrollment_tokens (
    id TEXT PRIMARY KEY NOT NULL CHECK(length(id) BETWEEN 1 AND 128),
    token_hash TEXT NOT NULL UNIQUE,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    expires_at INTEGER NOT NULL,
    used_at INTEGER,
    used_by_node_id TEXT
) STRICT;

CREATE TABLE scheduled_scans (
    library_id TEXT PRIMARY KEY REFERENCES libraries(id) ON DELETE CASCADE,
    interval_minutes BIGINT NOT NULL CHECK(interval_minutes BETWEEN 5 AND 10080),
    enabled BIGINT NOT NULL CHECK(enabled IN (0,1)),
    next_run_at BIGINT NOT NULL,
    last_run_at BIGINT,
    last_outcome TEXT NOT NULL DEFAULT 'never'
, last_request_key TEXT NOT NULL DEFAULT '');

CREATE INDEX scheduled_scans_due ON scheduled_scans(enabled,next_run_at);

CREATE TABLE user_groups (id TEXT PRIMARY KEY NOT NULL, name TEXT NOT NULL UNIQUE CHECK(length(name) BETWEEN 1 AND 80), is_default INTEGER NOT NULL DEFAULT 0) STRICT;

CREATE TABLE user_group_members (group_id TEXT NOT NULL REFERENCES user_groups(id) ON DELETE CASCADE, user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE, PRIMARY KEY(group_id,user_id)) STRICT;

CREATE INDEX user_group_members_user ON user_group_members(user_id);

CREATE TABLE user_group_libraries (group_id TEXT NOT NULL REFERENCES user_groups(id) ON DELETE CASCADE, library_id TEXT NOT NULL REFERENCES libraries(id) ON DELETE CASCADE, PRIMARY KEY(group_id,library_id)) STRICT;

CREATE TABLE api_keys (id TEXT PRIMARY KEY NOT NULL, name TEXT NOT NULL CHECK(length(name) BETWEEN 1 AND 80), user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE, token_hash TEXT NOT NULL UNIQUE, created_at INTEGER NOT NULL, expires_at INTEGER NOT NULL, revoked_at INTEGER, scope TEXT NOT NULL CHECK(scope='catalog:read')) STRICT;

CREATE INDEX api_keys_owner ON api_keys(user_id);

CREATE TABLE api_key_libraries (key_id TEXT NOT NULL REFERENCES api_keys(id) ON DELETE CASCADE, library_id TEXT NOT NULL REFERENCES libraries(id) ON DELETE CASCADE, PRIMARY KEY(key_id,library_id)) STRICT;

CREATE TABLE backup_schedule (
    singleton BIGINT PRIMARY KEY CHECK(singleton = 1),
    enabled BIGINT NOT NULL DEFAULT 0 CHECK(enabled IN (0,1)),
    interval_minutes BIGINT NOT NULL DEFAULT 1440 CHECK(interval_minutes BETWEEN 60 AND 10080),
    next_due_at BIGINT NOT NULL DEFAULT 0,
    last_started_at BIGINT,
    last_completed_at BIGINT,
    last_outcome TEXT NOT NULL DEFAULT 'never' CHECK(last_outcome IN ('never','running','succeeded','failed','interrupted'))
);

CREATE TABLE user_preferences (
    user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    preferences_json TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE login_throttle (
    throttle_key TEXT PRIMARY KEY NOT NULL,
    window_started INTEGER NOT NULL,
    attempts INTEGER NOT NULL
) STRICT;

CREATE INDEX login_throttle_window ON login_throttle(window_started);

CREATE TABLE server_settings (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    settings_json TEXT NOT NULL
);

CREATE TABLE user_avatars (
    user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    sha256 TEXT NOT NULL REFERENCES assets(sha256)
) STRICT;
CREATE INDEX user_avatars_asset ON user_avatars(sha256);

CREATE TABLE "work_feedback" (
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    work_id TEXT NOT NULL REFERENCES inventory_works(id) ON DELETE CASCADE,
    favorite INTEGER NOT NULL DEFAULT 0 CHECK(favorite IN (0,1)),
    rating REAL CHECK(rating BETWEEN 0.5 AND 5 AND rating * 2 = CAST(rating * 2 AS INTEGER)),
    PRIMARY KEY(user_id,work_id)
);

CREATE TABLE extension_publishers (
    id TEXT PRIMARY KEY,
    public_key TEXT NOT NULL,
    active BIGINT NOT NULL DEFAULT 1 CHECK(active IN (0,1))
);

CREATE TABLE extensions (
    id TEXT PRIMARY KEY,
    publisher_id TEXT NOT NULL REFERENCES extension_publishers(id),
    package_json TEXT NOT NULL,
    enabled BIGINT NOT NULL DEFAULT 0 CHECK(enabled IN (0,1)),
    generation TEXT NOT NULL
, all_libraries INTEGER NOT NULL DEFAULT 0 CHECK(all_libraries IN (0,1)));

CREATE TABLE extension_grants (
    extension_id TEXT NOT NULL REFERENCES extensions(id) ON DELETE CASCADE,
    library_id TEXT NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    PRIMARY KEY(extension_id,library_id)
);

CREATE TABLE extension_runs (
    id TEXT PRIMARY KEY,
    extension_id TEXT NOT NULL,
    library_id TEXT NOT NULL,
    work_id TEXT NOT NULL,
    actor_id TEXT NOT NULL,
    started_at BIGINT NOT NULL,
    duration_ms BIGINT NOT NULL DEFAULT 0,
    status TEXT NOT NULL,
    result_json TEXT NOT NULL DEFAULT '{}'
);

CREATE INDEX extension_runs_recent ON extension_runs(started_at);

CREATE TABLE extension_audit (
    id TEXT PRIMARY KEY,
    actor_id TEXT NOT NULL,
    extension_id TEXT NOT NULL,
    action TEXT NOT NULL,
    created_at BIGINT NOT NULL
);

CREATE TABLE "libraries" (
 id TEXT PRIMARY KEY NOT NULL,
 name TEXT NOT NULL CHECK(length(name) BETWEEN 1 AND 100),
 kind TEXT NOT NULL CHECK(kind IN ('films','series','music')),
 node_id TEXT NOT NULL REFERENCES media_nodes(id), root TEXT NOT NULL,
 enabled INTEGER NOT NULL CHECK(enabled IN (0,1)), excluded_patterns TEXT NOT NULL DEFAULT ''
, options_json TEXT NOT NULL DEFAULT '{}', next_metadata_refresh_at INTEGER NOT NULL DEFAULT 0) STRICT;

CREATE TABLE music_tracks (
 id TEXT PRIMARY KEY NOT NULL, library_id TEXT NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
 root_snapshot TEXT NOT NULL, relative_path TEXT NOT NULL, size INTEGER NOT NULL, modified_ns TEXT NOT NULL,
 title TEXT NOT NULL, artist TEXT NOT NULL, album TEXT NOT NULL, album_artist TEXT NOT NULL,
 track_number INTEGER, disc_number INTEGER, duration REAL, codec TEXT NOT NULL, mime TEXT NOT NULL,
 scan_id TEXT NOT NULL, available INTEGER NOT NULL DEFAULT 1, lyrics TEXT NOT NULL DEFAULT '', cover_path TEXT, cover_size INTEGER, cover_modified_ns TEXT, cover_mime TEXT,
 UNIQUE(library_id,root_snapshot,relative_path)
) STRICT;

CREATE INDEX music_tracks_library ON music_tracks(library_id,available);

CREATE TABLE music_scan_runs(library_id TEXT PRIMARY KEY REFERENCES libraries(id) ON DELETE CASCADE,scan_id TEXT NOT NULL) STRICT;

CREATE TABLE parental_policies (user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,maximum_age INTEGER NOT NULL CHECK(maximum_age BETWEEN 0 AND 21),allow_unrated INTEGER NOT NULL CHECK(allow_unrated IN (0,1)), blocked_tags_json TEXT NOT NULL DEFAULT '[]', access_schedule_json TEXT NOT NULL DEFAULT '[]') STRICT;

CREATE TABLE work_posters (
    work_id TEXT PRIMARY KEY REFERENCES inventory_works(id) ON DELETE CASCADE,
    sha256 TEXT NOT NULL REFERENCES assets(sha256),
    revision TEXT NOT NULL DEFAULT 'legacy'
) STRICT;
CREATE INDEX work_posters_asset ON work_posters(sha256);

CREATE TABLE personal_lists (id TEXT PRIMARY KEY,user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,name TEXT NOT NULL CHECK(length(name) BETWEEN 1 AND 100), visibility TEXT NOT NULL DEFAULT 'private' CHECK(visibility IN ('private','shared','public'))) STRICT;

CREATE TABLE personal_list_items (list_id TEXT NOT NULL REFERENCES personal_lists(id) ON DELETE CASCADE,work_id TEXT NOT NULL REFERENCES inventory_works(id) ON DELETE CASCADE,added_at INTEGER NOT NULL DEFAULT(unixepoch()), position INTEGER NOT NULL DEFAULT 0,PRIMARY KEY(list_id,work_id)) STRICT;

CREATE TABLE media_groups (id TEXT PRIMARY KEY,label TEXT NOT NULL) STRICT;

CREATE TABLE media_group_members (work_id TEXT PRIMARY KEY REFERENCES inventory_works(id) ON DELETE CASCADE,group_id TEXT NOT NULL REFERENCES media_groups(id) ON DELETE CASCADE) STRICT;

CREATE TABLE extension_settings (
    extension_id TEXT PRIMARY KEY REFERENCES extensions(id) ON DELETE CASCADE,
    settings_json TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE extension_repositories (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    url TEXT NOT NULL UNIQUE,
    manifest_json TEXT NOT NULL,
    refreshed_at INTEGER NOT NULL
) STRICT;

CREATE TABLE user_media_policies (user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,allow_media_download INTEGER NOT NULL DEFAULT 1 CHECK(allow_media_download IN (0,1))) STRICT;

CREATE TABLE parental_blocked_tags (user_id TEXT NOT NULL REFERENCES parental_policies(user_id) ON DELETE CASCADE,tag_json TEXT NOT NULL,PRIMARY KEY(user_id,tag_json)) STRICT;

CREATE TABLE library_artwork (
    library_id TEXT NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK(kind IN ('poster','backdrop')),
    sha256 TEXT NOT NULL REFERENCES assets(sha256),
    revision TEXT NOT NULL,
    PRIMARY KEY(library_id, kind)
) STRICT;
CREATE INDEX library_artwork_asset ON library_artwork(sha256);

CREATE INDEX media_group_members_group_work ON media_group_members(group_id,work_id);

CREATE TABLE work_image_entries (id TEXT PRIMARY KEY,work_id TEXT NOT NULL REFERENCES inventory_works(id) ON DELETE CASCADE,kind TEXT NOT NULL,position INTEGER NOT NULL,width INTEGER NOT NULL,height INTEGER NOT NULL, language TEXT, provider TEXT, fingerprint TEXT, source_url_hash TEXT) STRICT;

CREATE INDEX work_images_owner ON work_image_entries(work_id,kind,position,id);

CREATE TABLE work_image_data (
    image_id TEXT NOT NULL REFERENCES work_image_entries(id) ON DELETE CASCADE,
    width INTEGER NOT NULL,
    mime TEXT NOT NULL,
    sha256 TEXT NOT NULL REFERENCES assets(sha256),
    PRIMARY KEY(image_id, width)
) STRICT;
CREATE INDEX work_image_data_asset ON work_image_data(sha256);

CREATE TABLE work_image_hidden (work_id TEXT NOT NULL REFERENCES inventory_works(id) ON DELETE CASCADE,image_id TEXT NOT NULL,PRIMARY KEY(work_id,image_id)) STRICT;

CREATE TABLE work_image_order (work_id TEXT NOT NULL REFERENCES inventory_works(id) ON DELETE CASCADE,image_id TEXT NOT NULL,position INTEGER NOT NULL,PRIMARY KEY(work_id,image_id)) STRICT;

CREATE TABLE poster_previews (
    work_id TEXT NOT NULL REFERENCES work_posters(work_id) ON DELETE CASCADE,
    width INTEGER NOT NULL,
    revision TEXT NOT NULL,
    mime TEXT NOT NULL,
    sha256 TEXT NOT NULL REFERENCES assets(sha256),
    PRIMARY KEY(work_id, width)
) STRICT;
CREATE INDEX poster_previews_asset ON poster_previews(sha256);

CREATE INDEX inventory_display_sort ON inventory_works(kind,display_title_sort,id);

CREATE INDEX inventory_library_display_sort ON inventory_works(library_id,kind,display_title_sort,id);

CREATE TABLE playlist_music_items(list_id TEXT NOT NULL REFERENCES personal_lists(id) ON DELETE CASCADE,track_id TEXT NOT NULL REFERENCES music_tracks(id) ON DELETE CASCADE,position INTEGER NOT NULL,PRIMARY KEY(list_id,track_id)) STRICT;

CREATE TABLE playlist_shares(list_id TEXT NOT NULL REFERENCES personal_lists(id) ON DELETE CASCADE,user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,PRIMARY KEY(list_id,user_id)) STRICT;

CREATE INDEX playlist_shares_user ON playlist_shares(user_id,list_id);

CREATE TABLE playlist_policy(singleton INTEGER PRIMARY KEY CHECK(singleton=1),allow_public INTEGER NOT NULL DEFAULT 0 CHECK(allow_public IN (0,1)),allow_sharing INTEGER NOT NULL DEFAULT 0 CHECK(allow_sharing IN (0,1))) STRICT;

CREATE TABLE library_folders (
    library_id TEXT PRIMARY KEY NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    parent_id TEXT NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    active INTEGER NOT NULL DEFAULT 1 CHECK(active IN (0,1)),
    CHECK(library_id != parent_id)
);

CREATE INDEX library_folders_parent ON library_folders(parent_id, active);

CREATE VIEW effective_library_permissions AS
WITH direct_grants AS (
    SELECT library_id,user_id FROM library_permissions
    UNION SELECT l.library_id,m.user_id FROM user_group_libraries l JOIN user_group_members m ON m.group_id=l.group_id
    UNION SELECT l.library_id,u.id FROM user_group_libraries l JOIN user_groups g ON g.id=l.group_id CROSS JOIN users u WHERE g.is_default=1
)
SELECT d.library_id,d.user_id FROM direct_grants d WHERE NOT EXISTS(SELECT 1 FROM library_folders f WHERE f.library_id=d.library_id)
UNION SELECT f.library_id,d.user_id FROM direct_grants d JOIN library_folders f ON f.parent_id=d.library_id AND f.active=1;

CREATE TABLE extension_avatar_packs (
 id TEXT PRIMARY KEY, extension_id TEXT NOT NULL REFERENCES extensions(id) ON DELETE CASCADE,
 name TEXT NOT NULL, source TEXT NOT NULL
);

CREATE TABLE extension_avatar_images (
    id TEXT PRIMARY KEY,
    pack_id TEXT NOT NULL REFERENCES extension_avatar_packs(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    sha256 TEXT NOT NULL REFERENCES assets(sha256)
) STRICT;
CREATE INDEX extension_avatar_images_pack ON extension_avatar_images(pack_id);
CREATE INDEX extension_avatar_images_asset ON extension_avatar_images(sha256);

CREATE TABLE inventory_edition_metadata (
    edition_id TEXT PRIMARY KEY REFERENCES inventory_editions(id) ON DELETE CASCADE,
    edition_key TEXT NOT NULL DEFAULT 'standard',
    label TEXT NOT NULL DEFAULT 'Édition locale'
) STRICT;

CREATE TABLE inventory_version_metadata (
    version_id TEXT PRIMARY KEY REFERENCES inventory_versions(id) ON DELETE CASCADE,
    version_key TEXT NOT NULL DEFAULT 'default',
    label TEXT NOT NULL DEFAULT 'Version locale'
) STRICT;

CREATE TABLE media_work_external_ids (
    work_id TEXT NOT NULL REFERENCES inventory_works(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    provider TEXT NOT NULL,
    external_id TEXT NOT NULL,
    PRIMARY KEY (work_id, provider)
) STRICT;

CREATE INDEX media_work_external_ids_lookup ON media_work_external_ids(kind, provider, external_id, work_id);

CREATE TABLE media_match_candidates (
    id TEXT PRIMARY KEY,
    left_work_id TEXT NOT NULL REFERENCES inventory_works(id) ON DELETE CASCADE,
    right_work_id TEXT NOT NULL REFERENCES inventory_works(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    score INTEGER NOT NULL,
    reason TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending', 'confirmed', 'rejected', 'undone')),
    merge_id TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE(left_work_id, right_work_id)
) STRICT;

CREATE INDEX media_match_candidates_status ON media_match_candidates(status, score DESC, id);

CREATE TABLE media_merge_history (
    id TEXT PRIMARY KEY,
    group_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    origin TEXT NOT NULL CHECK(origin IN ('auto', 'manual', 'candidate')),
    candidate_id TEXT,
    snapshot_json TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    undone_at INTEGER
) STRICT;

CREATE INDEX media_merge_history_group ON media_merge_history(group_id, created_at DESC);

CREATE TRIGGER inventory_works_summary_insert AFTER INSERT ON inventory_works
WHEN NEW.display_title_sort = '' AND NEW.title_sort != '' BEGIN
    UPDATE inventory_works
    SET display_title_sort = lower(COALESCE(json_extract(NEW.manual_json, '$.title'), NEW.title_sort)),
        effective_year = COALESCE(CAST(json_extract(NEW.manual_json, '$.year') AS INTEGER), NEW.year),
        genres_csv = COALESCE(
            (SELECT '|' || group_concat(lower(trim(value)), '|') || '|'
             FROM json_each(CASE
                 WHEN json_valid(COALESCE(NEW.manual_json, NEW.editorial_json))
                      AND json_type(COALESCE(NEW.manual_json, NEW.editorial_json), '$.genres') = 'array'
                 THEN json_extract(COALESCE(NEW.manual_json, NEW.editorial_json), '$.genres')
                 ELSE '[]'
             END)
             WHERE trim(value) != ''),
            '|'
        )
    WHERE id = NEW.id;
END;

CREATE TRIGGER inventory_works_summary_update AFTER UPDATE OF title, title_sort, year, manual_json, editorial_json ON inventory_works
WHEN NEW.display_title_sort != lower(COALESCE(json_extract(NEW.manual_json, '$.title'), NEW.title_sort))
  OR NEW.effective_year IS NOT COALESCE(CAST(json_extract(NEW.manual_json, '$.year') AS INTEGER), NEW.year)
  OR OLD.manual_json IS NOT NEW.manual_json
  OR OLD.editorial_json IS NOT NEW.editorial_json
BEGIN
    UPDATE inventory_works
    SET display_title_sort = lower(COALESCE(json_extract(NEW.manual_json, '$.title'), NEW.title_sort)),
        effective_year = COALESCE(CAST(json_extract(NEW.manual_json, '$.year') AS INTEGER), NEW.year),
        genres_csv = COALESCE(
            (SELECT '|' || group_concat(lower(trim(value)), '|') || '|'
             FROM json_each(CASE
                 WHEN json_valid(COALESCE(NEW.manual_json, NEW.editorial_json))
                      AND json_type(COALESCE(NEW.manual_json, NEW.editorial_json), '$.genres') = 'array'
                 THEN json_extract(COALESCE(NEW.manual_json, NEW.editorial_json), '$.genres')
                 ELSE '[]'
             END)
             WHERE trim(value) != ''),
            '|'
        )
    WHERE id = NEW.id;
END;

CREATE TRIGGER inventory_assets_avail_insert AFTER INSERT ON inventory_assets
WHEN NEW.available = 1 BEGIN
    UPDATE inventory_works SET has_available_asset = 1
    WHERE id IN (
        SELECT e.work_id FROM inventory_sources s
        JOIN inventory_versions v ON v.id = s.version_id
        JOIN inventory_editions e ON e.id = v.edition_id
        WHERE s.id = NEW.source_id
    ) AND has_available_asset = 0;

    UPDATE inventory_works SET has_available_asset = 1
    WHERE id IN (
        SELECT se.series_id FROM inventory_sources s
        JOIN inventory_versions v ON v.id = s.version_id
        JOIN inventory_editions e ON e.id = v.edition_id
        JOIN inventory_episodes ep ON ep.work_id = e.work_id
        JOIN inventory_seasons se ON se.id = ep.season_id
        WHERE s.id = NEW.source_id
    ) AND has_available_asset = 0;
END;

CREATE TRIGGER inventory_episodes_avail_insert AFTER INSERT ON inventory_episodes BEGIN
    UPDATE inventory_works SET has_available_asset = 1
    WHERE id = (SELECT series_id FROM inventory_seasons WHERE id = NEW.season_id)
      AND has_available_asset = 0
      AND EXISTS (SELECT 1 FROM inventory_works ew WHERE ew.id = NEW.work_id AND ew.has_available_asset = 1);
END;

CREATE TRIGGER inventory_assets_avail_update AFTER UPDATE OF available, source_id ON inventory_assets BEGIN
    UPDATE inventory_works
    SET has_available_asset = CASE WHEN EXISTS(
        SELECT 1 FROM inventory_editions e
        JOIN inventory_versions v ON v.edition_id = e.id
        JOIN inventory_sources s ON s.version_id = v.id
        JOIN inventory_assets a ON a.source_id = s.id
        WHERE e.work_id = inventory_works.id AND a.available = 1
    ) THEN 1 ELSE 0 END
    WHERE id IN (
        SELECT e.work_id FROM inventory_sources s
        JOIN inventory_versions v ON v.id = s.version_id
        JOIN inventory_editions e ON e.id = v.edition_id
        WHERE s.id IN (OLD.source_id, NEW.source_id)
    );

    UPDATE inventory_works
    SET has_available_asset = CASE WHEN EXISTS(
        SELECT 1 FROM inventory_seasons se
        JOIN inventory_episodes ep ON ep.season_id = se.id
        JOIN inventory_editions e ON e.work_id = ep.work_id
        JOIN inventory_versions v ON v.edition_id = e.id
        JOIN inventory_sources s ON s.version_id = v.id
        JOIN inventory_assets a ON a.source_id = s.id
        WHERE se.series_id = inventory_works.id AND a.available = 1
    ) THEN 1 ELSE 0 END
    WHERE id IN (
        SELECT se.series_id FROM inventory_sources s
        JOIN inventory_versions v ON v.id = s.version_id
        JOIN inventory_editions e ON e.id = v.edition_id
        JOIN inventory_episodes ep ON ep.work_id = e.work_id
        JOIN inventory_seasons se ON se.id = ep.season_id
        WHERE s.id IN (OLD.source_id, NEW.source_id)
    );
END;

CREATE TRIGGER inventory_assets_avail_delete AFTER DELETE ON inventory_assets BEGIN
    UPDATE inventory_works
    SET has_available_asset = CASE WHEN EXISTS(
        SELECT 1 FROM inventory_editions e
        JOIN inventory_versions v ON v.edition_id = e.id
        JOIN inventory_sources s ON s.version_id = v.id
        JOIN inventory_assets a ON a.source_id = s.id
        WHERE e.work_id = inventory_works.id AND a.available = 1
    ) THEN 1 ELSE 0 END
    WHERE id IN (
        SELECT e.work_id FROM inventory_sources s
        JOIN inventory_versions v ON v.id = s.version_id
        JOIN inventory_editions e ON e.id = v.edition_id
        WHERE s.id = OLD.source_id
    );

    UPDATE inventory_works
    SET has_available_asset = CASE WHEN EXISTS(
        SELECT 1 FROM inventory_seasons se
        JOIN inventory_episodes ep ON ep.season_id = se.id
        JOIN inventory_editions e ON e.work_id = ep.work_id
        JOIN inventory_versions v ON v.edition_id = e.id
        JOIN inventory_sources s ON s.version_id = v.id
        JOIN inventory_assets a ON a.source_id = s.id
        WHERE se.series_id = inventory_works.id AND a.available = 1
    ) THEN 1 ELSE 0 END
    WHERE id IN (
        SELECT se.series_id FROM inventory_sources s
        JOIN inventory_versions v ON v.id = s.version_id
        JOIN inventory_editions e ON e.id = v.edition_id
        JOIN inventory_episodes ep ON ep.work_id = e.work_id
        JOIN inventory_seasons se ON se.id = ep.season_id
        WHERE s.id = OLD.source_id
    );
END;

CREATE TRIGGER inventory_tracks_summary_insert AFTER INSERT ON inventory_tracks BEGIN
    UPDATE inventory_works
    SET max_height = CASE
            WHEN json_extract(NEW.metadata_json, '$.codec_type') = 'video'
                 AND COALESCE(CAST(json_extract(NEW.metadata_json, '$.height') AS INTEGER), 0) > max_height
            THEN CAST(json_extract(NEW.metadata_json, '$.height') AS INTEGER)
            ELSE max_height
        END,
        codecs_csv = CASE
            WHEN json_extract(NEW.metadata_json, '$.codec_name') IS NOT NULL
                 AND instr(codecs_csv, ',' || lower(json_extract(NEW.metadata_json, '$.codec_name')) || ',') = 0
            THEN codecs_csv || lower(json_extract(NEW.metadata_json, '$.codec_name')) || ','
            ELSE codecs_csv
        END,
        languages_csv = CASE
            WHEN json_extract(NEW.metadata_json, '$.tags.language') IS NOT NULL
                 AND instr(languages_csv, ',' || lower(json_extract(NEW.metadata_json, '$.tags.language')) || ',') = 0
            THEN languages_csv || lower(json_extract(NEW.metadata_json, '$.tags.language')) || ','
            ELSE languages_csv
        END
    WHERE id IN (
        SELECT e.work_id FROM inventory_assets a
        JOIN inventory_sources s ON s.id = a.source_id
        JOIN inventory_versions v ON v.id = s.version_id
        JOIN inventory_editions e ON e.id = v.edition_id
        WHERE a.id = NEW.asset_id
    );
END;

CREATE INDEX inventory_works_global_sort ON inventory_works(kind, display_title_sort, id);

CREATE INDEX inventory_works_lib_display_sort ON inventory_works(library_id, kind, display_title_sort, id);

CREATE INDEX inventory_works_lib_avail_sort ON inventory_works(library_id, kind, has_available_asset, display_title_sort, id);

CREATE INDEX inventory_works_global_avail_sort ON inventory_works(kind, has_available_asset, display_title_sort, id);

CREATE INDEX inventory_works_avail_sort ON inventory_works(has_available_asset, display_title_sort, id, library_id, kind);

CREATE INDEX inventory_works_global_year_sort ON inventory_works(kind, effective_year, display_title_sort, id);

CREATE INDEX inventory_works_global_added_sort ON inventory_works(kind, added_at, display_title_sort, id);

CREATE INDEX inventory_works_fts_sort ON inventory_works(id, library_id, kind, has_available_asset, display_title_sort, effective_year, max_height, codecs_csv, languages_csv, genres_csv);

CREATE INDEX inventory_works_filter_sort ON inventory_works(kind, has_available_asset, effective_year, max_height, display_title_sort, id, library_id, codecs_csv, languages_csv, genres_csv);

CREATE INDEX inventory_sources_library_root ON inventory_sources(library_id, root_snapshot);

CREATE VIRTUAL TABLE inventory_works_fts USING fts5(
    work_id UNINDEXED,
    search_text,
    tokenize = 'trigram'
);

CREATE TRIGGER inventory_works_fts_insert AFTER INSERT ON inventory_works BEGIN
    INSERT INTO inventory_works_fts(work_id, search_text)
    VALUES (NEW.id, COALESCE(NULLIF(NEW.display_title_sort, ''), NEW.title_sort));
END;

CREATE TRIGGER inventory_works_fts_update AFTER UPDATE OF title_sort, display_title_sort, manual_json ON inventory_works
WHEN COALESCE(NULLIF(OLD.display_title_sort, ''), OLD.title_sort) != COALESCE(NULLIF(NEW.display_title_sort, ''), lower(COALESCE(json_extract(NEW.manual_json, '$.title'), NEW.title_sort)))
BEGIN
    DELETE FROM inventory_works_fts WHERE length(COALESCE(NULLIF(OLD.display_title_sort, ''), OLD.title_sort)) >= 3 AND search_text MATCH '"' || replace(COALESCE(NULLIF(OLD.display_title_sort, ''), OLD.title_sort), '"', '""') || '"' AND work_id = OLD.id;
    DELETE FROM inventory_works_fts WHERE length(COALESCE(NULLIF(OLD.display_title_sort, ''), OLD.title_sort)) < 3 AND work_id = OLD.id;
    INSERT INTO inventory_works_fts(work_id, search_text)
    VALUES (NEW.id, COALESCE(NULLIF(NEW.display_title_sort, ''), lower(COALESCE(json_extract(NEW.manual_json, '$.title'), NEW.title_sort))));
END;

CREATE TRIGGER inventory_works_fts_delete AFTER DELETE ON inventory_works BEGIN
    DELETE FROM inventory_works_fts WHERE length(COALESCE(NULLIF(OLD.display_title_sort, ''), OLD.title_sort)) >= 3 AND search_text MATCH '"' || replace(COALESCE(NULLIF(OLD.display_title_sort, ''), OLD.title_sort), '"', '""') || '"' AND work_id = OLD.id;
    DELETE FROM inventory_works_fts WHERE length(COALESCE(NULLIF(OLD.display_title_sort, ''), OLD.title_sort)) < 3 AND work_id = OLD.id;
END;

CREATE TABLE inventory_cache_generation (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    gen INTEGER NOT NULL DEFAULT 1
);

CREATE TRIGGER cache_gen_works_ins AFTER INSERT ON inventory_works BEGIN
    UPDATE inventory_cache_generation SET gen = gen + 1 WHERE id = 1;
END;

CREATE TRIGGER cache_gen_works_upd AFTER UPDATE ON inventory_works BEGIN
    UPDATE inventory_cache_generation SET gen = gen + 1 WHERE id = 1;
END;

CREATE TRIGGER cache_gen_works_del AFTER DELETE ON inventory_works BEGIN
    UPDATE inventory_cache_generation SET gen = gen + 1 WHERE id = 1;
END;

CREATE TRIGGER cache_gen_assets_upd AFTER UPDATE OF available, source_id ON inventory_assets BEGIN
    UPDATE inventory_cache_generation SET gen = gen + 1 WHERE id = 1;
END;

CREATE TRIGGER cache_gen_libs_upd AFTER INSERT ON libraries BEGIN
    UPDATE inventory_cache_generation SET gen = gen + 1 WHERE id = 1;
END;

CREATE TRIGGER cache_gen_libs_chg AFTER UPDATE ON libraries BEGIN
    UPDATE inventory_cache_generation SET gen = gen + 1 WHERE id = 1;
END;

CREATE TRIGGER cache_gen_groups_ins AFTER INSERT ON media_group_members BEGIN
    UPDATE inventory_cache_generation SET gen = gen + 1 WHERE id = 1;
END;

CREATE TRIGGER cache_gen_groups_del AFTER DELETE ON media_group_members BEGIN
    UPDATE inventory_cache_generation SET gen = gen + 1 WHERE id = 1;
END;

CREATE TRIGGER cache_gen_parental_ins AFTER INSERT ON parental_policies BEGIN
    UPDATE inventory_cache_generation SET gen = gen + 1 WHERE id = 1;
END;

CREATE TRIGGER cache_gen_parental_upd AFTER UPDATE ON parental_policies BEGIN
    UPDATE inventory_cache_generation SET gen = gen + 1 WHERE id = 1;
END;

CREATE TRIGGER cache_gen_parental_del AFTER DELETE ON parental_policies BEGIN
    UPDATE inventory_cache_generation SET gen = gen + 1 WHERE id = 1;
END;

CREATE TRIGGER cache_gen_perms_ins AFTER INSERT ON library_permissions BEGIN
    UPDATE inventory_cache_generation SET gen = gen + 1 WHERE id = 1;
END;

CREATE TRIGGER cache_gen_perms_del AFTER DELETE ON library_permissions BEGIN
    UPDATE inventory_cache_generation SET gen = gen + 1 WHERE id = 1;
END;

CREATE TRIGGER cache_gen_images_ins AFTER INSERT ON inventory_images BEGIN
    UPDATE inventory_cache_generation SET gen = gen + 1 WHERE id = 1;
END;

CREATE TRIGGER cache_gen_images_upd AFTER UPDATE ON inventory_images BEGIN
    UPDATE inventory_cache_generation SET gen = gen + 1 WHERE id = 1;
END;

CREATE TRIGGER cache_gen_posters_ins AFTER INSERT ON work_posters BEGIN
    UPDATE inventory_cache_generation SET gen = gen + 1 WHERE id = 1;
END;

CREATE TRIGGER cache_gen_posters_upd AFTER UPDATE ON work_posters BEGIN
    UPDATE inventory_cache_generation SET gen = gen + 1 WHERE id = 1;
END;

CREATE TRIGGER cache_gen_posters_del AFTER DELETE ON work_posters BEGIN
    UPDATE inventory_cache_generation SET gen = gen + 1 WHERE id = 1;
END;

CREATE TABLE durable_tasks (
    id TEXT PRIMARY KEY NOT NULL,
    library_id TEXT NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    owner TEXT NOT NULL,
    idempotency_key TEXT NOT NULL,
    node_id TEXT NOT NULL REFERENCES media_nodes(id),
    category TEXT NOT NULL DEFAULT 'scan' CHECK(category IN ('scan','analyze','prepare','image')),
    target_id TEXT NOT NULL DEFAULT '',
    component TEXT NOT NULL DEFAULT '',
    width BIGINT NOT NULL DEFAULT 640 CHECK(width IN (320,640,1280)),
    result_json TEXT NOT NULL DEFAULT '{}',
    refresh_mode TEXT NOT NULL DEFAULT 'incremental' CHECK(refresh_mode IN ('incremental','new_only','missing_metadata','full')),
    status TEXT NOT NULL DEFAULT 'queued' CHECK(status IN ('queued','running','paused','completed','cancelled','error')),
    priority BIGINT NOT NULL DEFAULT 100 CHECK(priority BETWEEN 0 AND 1000),
    attempt BIGINT NOT NULL DEFAULT 0 CHECK(attempt>=0),
    failure_count BIGINT NOT NULL DEFAULT 0 CHECK(failure_count>=0),
    max_attempts BIGINT NOT NULL DEFAULT 3 CHECK(max_attempts BETWEEN 1 AND 1000000),
    progress BIGINT NOT NULL DEFAULT 0 CHECK(progress BETWEEN 0 AND 100),
    processed_items BIGINT NOT NULL DEFAULT 0 CHECK(processed_items>=0),
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    started_at BIGINT,
    finished_at BIGINT,
    lease_until BIGINT,
    available_at BIGINT NOT NULL,
    error TEXT NOT NULL DEFAULT '',
    UNIQUE(owner,idempotency_key)
);

CREATE INDEX durable_tasks_dispatch ON durable_tasks(node_id,status,priority DESC,created_at,id);

CREATE INDEX durable_tasks_expiry ON durable_tasks(status,lease_until);

CREATE INDEX durable_tasks_library ON durable_tasks(library_id,status);

CREATE INDEX durable_tasks_recent ON durable_tasks(created_at DESC,id);

CREATE INDEX durable_tasks_pending ON durable_tasks(status) WHERE status IN ('queued','running','paused');

CREATE INDEX durable_tasks_leased ON durable_tasks(lease_until,id) WHERE lease_until IS NOT NULL;

CREATE INDEX durable_tasks_leased_node ON durable_tasks(node_id) WHERE lease_until IS NOT NULL;

CREATE INDEX durable_tasks_leased_library ON durable_tasks(library_id) WHERE lease_until IS NOT NULL;

CREATE INDEX durable_tasks_ready ON durable_tasks(status,available_at,node_id) WHERE status='queued';

CREATE TABLE active_scan_runs (
    library_id TEXT PRIMARY KEY NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    scan_id TEXT NOT NULL,
    node_id TEXT NOT NULL REFERENCES media_nodes(id)
);

CREATE TABLE scan_batch_receipts (
    scan_id TEXT NOT NULL,
    sequence BIGINT NOT NULL CHECK(sequence>=0),
    PRIMARY KEY(scan_id,sequence)
);

CREATE INDEX work_image_provider_source ON work_image_entries(work_id,provider,source_url_hash);

CREATE TABLE artwork_image_sources (work_id TEXT NOT NULL REFERENCES inventory_works(id) ON DELETE CASCADE,source_url_hash TEXT NOT NULL,image_id TEXT NOT NULL REFERENCES work_image_entries(id) ON DELETE CASCADE,PRIMARY KEY(work_id,source_url_hash));

CREATE TABLE active_playbacks (
 id TEXT PRIMARY KEY,
 session_hash TEXT NOT NULL REFERENCES sessions(token_hash) ON DELETE CASCADE,
 work_id TEXT NOT NULL,
 position_seconds DOUBLE PRECISION NOT NULL,
 duration_seconds DOUBLE PRECISION NOT NULL,
 paused INTEGER NOT NULL CHECK(paused IN (0,1)),
 updated_at BIGINT NOT NULL
);

CREATE INDEX active_playbacks_session ON active_playbacks(session_hash);

CREATE INDEX active_playbacks_updated ON active_playbacks(updated_at);

CREATE TABLE system_task_schedules (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT NOT NULL,
    category TEXT NOT NULL,
    interval_minutes BIGINT NOT NULL CHECK(interval_minutes BETWEEN 5 AND 10080),
    enabled BIGINT NOT NULL CHECK(enabled IN (0,1)),
    next_due_at BIGINT NOT NULL,
    last_started_at BIGINT,
    last_completed_at BIGINT,
    last_outcome TEXT NOT NULL DEFAULT 'never' CHECK(last_outcome IN ('never','running','succeeded','failed','interrupted'))
, last_error_kind TEXT, last_error_scope TEXT, last_message TEXT);

CREATE TABLE extension_data_namespaces (
 extension_id TEXT PRIMARY KEY,
 publisher_id TEXT NOT NULL,
 publisher_key TEXT NOT NULL,
 entries BIGINT NOT NULL DEFAULT 0 CHECK(entries >= 0),
 bytes BIGINT NOT NULL DEFAULT 0 CHECK(bytes >= 0)
);

CREATE TABLE extension_data (
 extension_id TEXT NOT NULL REFERENCES extension_data_namespaces(extension_id),
 scope TEXT NOT NULL,
 scope_id TEXT NOT NULL,
 key TEXT NOT NULL,
 value TEXT NOT NULL,
 version TEXT NOT NULL,
 expires_at BIGINT,
 durability TEXT NOT NULL CHECK(durability IN ('cache','durable')),
 value_bytes BIGINT NOT NULL CHECK(value_bytes >= 0),
 sha256 TEXT NOT NULL,
 PRIMARY KEY(extension_id,scope,scope_id,key)
);

CREATE INDEX extension_data_expiry ON extension_data(extension_id,expires_at);

CREATE TABLE extension_objects (
    id TEXT PRIMARY KEY,
    extension_id TEXT NOT NULL,
    scope_kind TEXT NOT NULL,
    scope_id TEXT NOT NULL,
    size BIGINT NOT NULL CHECK(size BETWEEN 0 AND 16777216),
    received BIGINT NOT NULL DEFAULT 0 CHECK(received >= 0 AND received <= size),
    sha256 TEXT NOT NULL,
    durability TEXT NOT NULL CHECK(durability IN ('cache','durable')),
    expires_at BIGINT,
    upload_expires_at BIGINT NOT NULL,
    complete BIGINT NOT NULL DEFAULT 0 CHECK(complete IN (0,1))
);

CREATE INDEX extension_objects_owner ON extension_objects(extension_id,scope_kind,scope_id);

CREATE TABLE extension_object_chunks (
    object_id TEXT NOT NULL REFERENCES extension_objects(id) ON DELETE CASCADE,
    offset_bytes BIGINT NOT NULL CHECK(offset_bytes >= 0),
    payload BLOB NOT NULL CHECK(length(payload) <= 16384),
    sha256 TEXT NOT NULL,
    PRIMARY KEY(object_id,offset_bytes)
);

-- Default seeds
INSERT INTO media_nodes (id, name, kind, status, key_id, shared_secret, roots_json, paired_at, connection_mode)
VALUES ('local', 'Node local (Standalone)', 'local', 'connected', 'local', '', '[]', strftime('%s','now'), 'outbound');

INSERT INTO backup_schedule (singleton, enabled, interval_minutes, next_due_at, last_outcome)
VALUES (1, 0, 1440, 0, 'never');

INSERT INTO playlist_policy (singleton, allow_public, allow_sharing)
VALUES (1, 0, 0);

INSERT INTO inventory_cache_generation (id, gen)
VALUES (1, 1);

INSERT INTO extension_repositories (id, name, url, manifest_json, refreshed_at)
VALUES ('community-default', 'Aether Plugins', 'https://raw.githubusercontent.com/maelmoreau21/Aether-Plugins/main/repository/manifest.json', '{"apiVersion":1,"extensions":[]}', 0);

INSERT INTO system_task_schedules (id, name, description, category, interval_minutes, enabled, next_due_at) VALUES
('update-extensions', 'Mettre à jour les plugins', 'Vérifie les dépôts distants et actualise les catalogues d''extensions.', 'Extensions', 1440, 1, strftime('%s','now') + 86400),
('optimize-database', 'Optimiser la base de données', 'Exécute l''optimisation du moteur de base de données et des journaux de transactions.', 'Maintenance', 1440, 1, strftime('%s','now') + 86400),
('clean-sessions', 'Nettoyer les sessions expirées', 'Supprime les sessions obsolètes et les flux d''authentification expirés.', 'Maintenance', 360, 1, strftime('%s','now') + 21600),
('clean-cache', 'Nettoyer le cache et fichiers temporaires', 'Supprime les fichiers temporaires périmés des dossiers appartenant à Aether (jamais les médias).', 'Maintenance', 1440, 1, strftime('%s','now') + 86400),
('clean-playlists', 'Nettoyer les listes et collections', 'Retire des listes de lecture personnelles les éléments dont les médias ont été supprimés.', 'Maintenance', 1440, 1, strftime('%s','now') + 86400);

INSERT INTO inventory_works_unicode_migration (completed_at) VALUES ('1970-01-01 00:00:00');
