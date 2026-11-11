-- Aether Core schema (sqlite). Applied once, by `database::initialize_fresh_schema`, to an empty
-- database. There is no migration chain: a change to the schema edits this file and databases
-- created before it are recreated.


-- Fresh SQLite schema. Do not apply to an existing database.

CREATE TABLE assets (
    sha256 TEXT PRIMARY KEY CHECK(length(sha256)=64),
    size INTEGER NOT NULL CHECK(size>0),
    mime TEXT NOT NULL CHECK(length(mime) BETWEEN 3 AND 100),
    created_at INTEGER NOT NULL,
    orphaned_at INTEGER
) STRICT;

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

CREATE TABLE library_permissions (
    library_id TEXT NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    PRIMARY KEY(library_id, user_id)
) STRICT;

CREATE TABLE library_initialization (
    singleton INTEGER PRIMARY KEY CHECK(singleton = 1)
) STRICT;

CREATE TABLE inventory_works (id TEXT PRIMARY KEY, library_id TEXT NOT NULL REFERENCES libraries(id), identity_key TEXT NOT NULL, title TEXT NOT NULL, year INTEGER, external_ids_json TEXT NOT NULL, editorial_json TEXT NOT NULL DEFAULT '{}', title_sort TEXT NOT NULL DEFAULT '', kind TEXT NOT NULL DEFAULT 'film', added_at INTEGER, manual_json TEXT, minimum_age INTEGER CHECK(minimum_age BETWEEN 0 AND 21), display_title_sort TEXT NOT NULL DEFAULT '', effective_year INTEGER, max_height INTEGER NOT NULL DEFAULT 0, codecs_csv TEXT NOT NULL DEFAULT ',', languages_csv TEXT NOT NULL DEFAULT ',', genres_csv TEXT NOT NULL DEFAULT '|', has_available_asset INTEGER NOT NULL DEFAULT 0 CHECK(has_available_asset IN (0, 1)), season_count_cached INTEGER NOT NULL DEFAULT 0, episode_count_cached INTEGER NOT NULL DEFAULT 0, UNIQUE(library_id,identity_key)) STRICT;

CREATE TABLE inventory_editions (id TEXT PRIMARY KEY, work_id TEXT NOT NULL REFERENCES inventory_works(id)) STRICT;

CREATE TABLE inventory_versions (id TEXT PRIMARY KEY, edition_id TEXT NOT NULL REFERENCES inventory_editions(id)) STRICT;

CREATE TABLE inventory_sources (root_snapshot TEXT NOT NULL, id TEXT PRIMARY KEY, version_id TEXT NOT NULL REFERENCES inventory_versions(id), library_id TEXT NOT NULL REFERENCES libraries(id)) STRICT;

CREATE TABLE inventory_assets (id TEXT PRIMARY KEY, source_id TEXT NOT NULL REFERENCES inventory_sources(id), library_id TEXT NOT NULL REFERENCES libraries(id), relative_path TEXT NOT NULL, size INTEGER NOT NULL, modified_ns TEXT NOT NULL, mime TEXT NOT NULL, probe_json TEXT NOT NULL, available INTEGER NOT NULL CHECK(available IN (0,1)), UNIQUE(library_id,relative_path)) STRICT;

CREATE TABLE inventory_tracks (id TEXT PRIMARY KEY, asset_id TEXT NOT NULL REFERENCES inventory_assets(id), stream_index INTEGER NOT NULL, metadata_json TEXT NOT NULL, UNIQUE(asset_id,stream_index)) STRICT;

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

CREATE TABLE user_groups (id TEXT PRIMARY KEY NOT NULL, name TEXT NOT NULL UNIQUE CHECK(length(name) BETWEEN 1 AND 80), is_default INTEGER NOT NULL DEFAULT 0) STRICT;

CREATE TABLE user_group_members (group_id TEXT NOT NULL REFERENCES user_groups(id) ON DELETE CASCADE, user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE, PRIMARY KEY(group_id,user_id)) STRICT;

CREATE TABLE user_group_libraries (group_id TEXT NOT NULL REFERENCES user_groups(id) ON DELETE CASCADE, library_id TEXT NOT NULL REFERENCES libraries(id) ON DELETE CASCADE, PRIMARY KEY(group_id,library_id)) STRICT;

CREATE TABLE api_keys (id TEXT PRIMARY KEY NOT NULL, name TEXT NOT NULL CHECK(length(name) BETWEEN 1 AND 80), user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE, token_hash TEXT NOT NULL UNIQUE, created_at INTEGER NOT NULL, expires_at INTEGER NOT NULL, revoked_at INTEGER, scope TEXT NOT NULL CHECK(scope='catalog:read')) STRICT;

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

CREATE TABLE server_settings (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    settings_json TEXT NOT NULL
);

CREATE TABLE user_avatars (
    user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    sha256 TEXT NOT NULL REFERENCES assets(sha256)
) STRICT;

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

CREATE TABLE extension_audit (
    id TEXT PRIMARY KEY,
    actor_id TEXT NOT NULL,
    extension_id TEXT NOT NULL,
    action TEXT NOT NULL,
    created_at BIGINT NOT NULL
);
CREATE INDEX extension_audit_retention ON extension_audit(created_at,id);

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

CREATE TABLE music_scan_runs(library_id TEXT PRIMARY KEY REFERENCES libraries(id) ON DELETE CASCADE,scan_id TEXT NOT NULL) STRICT;

CREATE TABLE parental_policies (user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,maximum_age INTEGER NOT NULL CHECK(maximum_age BETWEEN 0 AND 21),allow_unrated INTEGER NOT NULL CHECK(allow_unrated IN (0,1)), blocked_tags_json TEXT NOT NULL DEFAULT '[]', access_schedule_json TEXT NOT NULL DEFAULT '[]') STRICT;

CREATE TABLE work_posters (
    work_id TEXT PRIMARY KEY REFERENCES inventory_works(id) ON DELETE CASCADE,
    sha256 TEXT NOT NULL REFERENCES assets(sha256),
    revision TEXT NOT NULL DEFAULT 'legacy'
) STRICT;

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

CREATE TABLE work_image_entries (id TEXT PRIMARY KEY,work_id TEXT NOT NULL REFERENCES inventory_works(id) ON DELETE CASCADE,kind TEXT NOT NULL,position INTEGER NOT NULL,width INTEGER NOT NULL,height INTEGER NOT NULL, language TEXT, provider TEXT, fingerprint TEXT, source_url_hash TEXT) STRICT;

CREATE TABLE work_image_data (
    image_id TEXT NOT NULL REFERENCES work_image_entries(id) ON DELETE CASCADE,
    width INTEGER NOT NULL,
    mime TEXT NOT NULL,
    sha256 TEXT NOT NULL REFERENCES assets(sha256),
    PRIMARY KEY(image_id, width)
) STRICT;

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

CREATE TABLE playlist_music_items(list_id TEXT NOT NULL REFERENCES personal_lists(id) ON DELETE CASCADE,track_id TEXT NOT NULL REFERENCES music_tracks(id) ON DELETE CASCADE,position INTEGER NOT NULL,PRIMARY KEY(list_id,track_id)) STRICT;

CREATE TABLE playlist_shares(list_id TEXT NOT NULL REFERENCES personal_lists(id) ON DELETE CASCADE,user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,PRIMARY KEY(list_id,user_id)) STRICT;

CREATE TABLE playlist_policy(singleton INTEGER PRIMARY KEY CHECK(singleton=1),allow_public INTEGER NOT NULL DEFAULT 0 CHECK(allow_public IN (0,1)),allow_sharing INTEGER NOT NULL DEFAULT 0 CHECK(allow_sharing IN (0,1))) STRICT;

CREATE TABLE library_folders (
    library_id TEXT PRIMARY KEY NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    parent_id TEXT NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    active INTEGER NOT NULL DEFAULT 1 CHECK(active IN (0,1)),
    CHECK(library_id != parent_id)
);

CREATE TABLE avatar_packs (
 id TEXT PRIMARY KEY, name TEXT NOT NULL, source TEXT NOT NULL
);
CREATE TABLE avatar_images (
 id TEXT PRIMARY KEY, pack_id TEXT NOT NULL REFERENCES avatar_packs(id) ON DELETE CASCADE,
 name TEXT NOT NULL, sha256 TEXT NOT NULL REFERENCES assets(sha256)
);
CREATE INDEX avatar_images_asset ON avatar_images(sha256);
CREATE INDEX avatar_images_pack ON avatar_images(pack_id);
CREATE TABLE avatar_policy (
 singleton INTEGER PRIMARY KEY CHECK(singleton=1),
 allow_custom INTEGER NOT NULL CHECK(allow_custom IN (0,1)),
 allow_gravatar INTEGER NOT NULL CHECK(allow_gravatar IN (0,1)),
 assign_on_create INTEGER NOT NULL CHECK(assign_on_create IN (0,1))
);

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

CREATE VIRTUAL TABLE inventory_works_fts USING fts5(
    work_id UNINDEXED,
    search_text,
    tokenize = 'trigram'
);

CREATE TABLE inventory_cache_generation (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    gen INTEGER NOT NULL DEFAULT 1
);

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

CREATE TABLE extension_object_chunks (
    object_id TEXT NOT NULL REFERENCES extension_objects(id) ON DELETE CASCADE,
    offset_bytes BIGINT NOT NULL CHECK(offset_bytes >= 0),
    payload BLOB NOT NULL CHECK(length(payload) <= 16384),
    sha256 TEXT NOT NULL,
    PRIMARY KEY(object_id,offset_bytes)
);

CREATE TABLE automation_events (
 id TEXT PRIMARY KEY, event_key TEXT NOT NULL UNIQUE, kind TEXT NOT NULL,
 library_id TEXT NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
 asset_id TEXT NOT NULL, fingerprint TEXT NOT NULL, payload_json TEXT NOT NULL,
 created_at BIGINT NOT NULL, dispatched_at BIGINT, cursor_extension TEXT NOT NULL DEFAULT ''
);

CREATE TABLE automation_observations (
 asset_id TEXT PRIMARY KEY, library_id TEXT NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
 fingerprint TEXT NOT NULL, revision BIGINT NOT NULL
);

CREATE TABLE automation_policies (
 extension_id TEXT NOT NULL REFERENCES extensions(id) ON DELETE CASCADE,
 library_id TEXT NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
 mode TEXT NOT NULL DEFAULT 'automatic' CHECK(mode IN ('automatic','manual','disabled','scheduled')),
 priority BIGINT NOT NULL DEFAULT 100 CHECK(priority BETWEEN 0 AND 1000),
 operation TEXT NOT NULL DEFAULT 'metadata.propose', config_revision TEXT NOT NULL DEFAULT '',
 algorithm_version TEXT NOT NULL DEFAULT '1', interval_seconds BIGINT NOT NULL DEFAULT 86400 CHECK(interval_seconds>=60),
 next_run_at BIGINT NOT NULL DEFAULT 0, max_concurrent BIGINT NOT NULL DEFAULT 1 CHECK(max_concurrent BETWEEN 1 AND 16),
 PRIMARY KEY(extension_id,library_id)
);

CREATE TABLE automation_provider_limits (
 provider TEXT PRIMARY KEY, next_allowed_at BIGINT NOT NULL DEFAULT 0,
 min_interval_seconds BIGINT NOT NULL DEFAULT 1 CHECK(min_interval_seconds>=0),
 max_concurrent BIGINT NOT NULL DEFAULT 2 CHECK(max_concurrent BETWEEN 1 AND 16)
);

CREATE TABLE automation_sweeps (
 id TEXT PRIMARY KEY, library_id TEXT NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
 extension_id TEXT NOT NULL DEFAULT '', kind TEXT NOT NULL, cursor_asset TEXT NOT NULL DEFAULT '',
 created_at BIGINT NOT NULL, completed_at BIGINT
);

CREATE TABLE durable_tasks (
    id TEXT PRIMARY KEY NOT NULL,
    library_id TEXT REFERENCES libraries(id) ON DELETE CASCADE,
    owner TEXT NOT NULL,
    idempotency_key TEXT NOT NULL,
    node_id TEXT NOT NULL REFERENCES media_nodes(id),
    category TEXT NOT NULL DEFAULT 'scan' CHECK(category IN ('scan','analyze','prepare','image','trickplay','system')),
    target_id TEXT NOT NULL DEFAULT '',
    component TEXT NOT NULL DEFAULT '',
    width BIGINT NOT NULL DEFAULT 640 CHECK((category='trickplay' AND width BETWEEN 160 AND 1920) OR width IN (320,640,1280)),
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
    CHECK((category='system' AND library_id IS NULL AND node_id='local') OR (category!='system' AND library_id IS NOT NULL)),
    UNIQUE(owner,idempotency_key)
);

CREATE TABLE automation_task_links (
 task_id TEXT PRIMARY KEY REFERENCES durable_tasks(id) ON DELETE CASCADE,
 event_id TEXT NOT NULL REFERENCES automation_events(id), extension_id TEXT NOT NULL,
 operation TEXT NOT NULL, fingerprint TEXT NOT NULL, generation TEXT NOT NULL, provider TEXT NOT NULL,
 UNIQUE(extension_id,fingerprint)
);

CREATE TABLE automation_dependencies (
 task_id TEXT NOT NULL REFERENCES durable_tasks(id) ON DELETE CASCADE,
 prerequisite_id TEXT NOT NULL REFERENCES durable_tasks(id) ON DELETE CASCADE,
 PRIMARY KEY(task_id,prerequisite_id), CHECK(task_id<>prerequisite_id)
);

CREATE TABLE automation_results (
 extension_id TEXT NOT NULL, asset_id TEXT NOT NULL, fingerprint TEXT NOT NULL,
 task_id TEXT NOT NULL REFERENCES durable_tasks(id), result_json TEXT NOT NULL, updated_at BIGINT NOT NULL,
 PRIMARY KEY(extension_id,asset_id)
);

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
    last_outcome TEXT NOT NULL DEFAULT 'never' CHECK(last_outcome IN ('never','queued','running','succeeded','failed','interrupted'))
, last_error_kind TEXT, last_error_scope TEXT, last_message TEXT);

CREATE TABLE manual_timeline_markers (
    asset_id TEXT NOT NULL REFERENCES inventory_assets(id) ON DELETE CASCADE,
    marker_type TEXT NOT NULL CHECK(marker_type IN ('intro','credits','recap','preview')),
    start_seconds REAL,
    end_seconds REAL,
    enabled INTEGER NOT NULL CHECK(enabled IN (0,1)),
    updated_by TEXT NOT NULL REFERENCES users(id),
    updated_at INTEGER NOT NULL,
    PRIMARY KEY(asset_id,marker_type),
    CHECK((enabled=0 AND start_seconds IS NULL AND end_seconds IS NULL) OR
          (enabled=1 AND start_seconds>=0 AND end_seconds>start_seconds))
) STRICT;

CREATE TABLE automatic_timeline_markers (
    asset_id TEXT NOT NULL REFERENCES inventory_assets(id) ON DELETE CASCADE,
    extension_id TEXT NOT NULL REFERENCES extensions(id) ON DELETE CASCADE,
    marker_type TEXT NOT NULL CHECK(marker_type IN ('intro','credits','recap','preview')),
    start_seconds REAL NOT NULL CHECK(start_seconds>=0),
    end_seconds REAL NOT NULL CHECK(end_seconds>start_seconds),
    confidence REAL NOT NULL CHECK(confidence>=0 AND confidence<=1),
    source_fingerprint TEXT NOT NULL,
    generation TEXT NOT NULL,
    task_id TEXT NOT NULL REFERENCES durable_tasks(id),
    updated_at INTEGER NOT NULL,
    PRIMARY KEY(asset_id,extension_id,marker_type)
) STRICT;

CREATE TABLE automatic_metadata_provenance (
    work_id TEXT NOT NULL,
    field_name TEXT NOT NULL CHECK(length(field_name) BETWEEN 1 AND 64),
    extension_id TEXT NOT NULL REFERENCES extensions(id) ON DELETE CASCADE,
    provider_id TEXT NOT NULL DEFAULT '',
    priority INTEGER NOT NULL CHECK(priority BETWEEN 0 AND 1000),
    value_json TEXT NOT NULL,
    source_fingerprint TEXT NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY(work_id,field_name)
) STRICT;

CREATE TABLE automation_provider_cache (
    cache_key TEXT PRIMARY KEY,
    provider TEXT NOT NULL,
    operation TEXT NOT NULL,
    query_hash TEXT NOT NULL,
    response_json TEXT NOT NULL,
    expires_at INTEGER NOT NULL,
    created_at INTEGER NOT NULL
) STRICT;


CREATE INDEX assets_orphaned ON assets(orphaned_at) WHERE orphaned_at IS NOT NULL;

CREATE INDEX sessions_user ON sessions(user_id);

CREATE INDEX library_permissions_user ON library_permissions(user_id, library_id);

CREATE INDEX inventory_sources_library ON inventory_sources(library_id);

CREATE INDEX inventory_images_work ON inventory_images(work_id,available,priority,relative_path);

CREATE INDEX inventory_works_library_sort ON inventory_works(library_id, title_sort, id);

CREATE INDEX inventory_works_title_sort ON inventory_works(title_sort, id);

CREATE INDEX inventory_editions_work ON inventory_editions(work_id);

CREATE INDEX inventory_versions_edition ON inventory_versions(edition_id);

CREATE INDEX inventory_sources_version ON inventory_sources(version_id);

CREATE INDEX inventory_assets_source_available ON inventory_assets(source_id, available);

CREATE INDEX scan_staging_files_scan_reused ON scan_staging_files(scan_id, reused);

CREATE INDEX scan_staging_files_scan_library ON scan_staging_files(scan_id, library_id);

CREATE INDEX inventory_images_library_available ON inventory_images(library_id, available);

CREATE INDEX inventory_works_library_kind ON inventory_works(library_id, kind);

CREATE INDEX inventory_seasons_series ON inventory_seasons(series_id, season_number);

CREATE INDEX inventory_episodes_season ON inventory_episodes(season_id, episode_number);

CREATE INDEX inventory_episodes_work ON inventory_episodes(work_id);

CREATE INDEX oidc_flows_expiry ON oidc_flows(expires_at);

CREATE INDEX playback_progress_recent ON playback_progress(user_id,updated_at DESC);

CREATE INDEX inventory_works_added_at ON inventory_works(added_at, id);

CREATE INDEX scheduled_scans_due ON scheduled_scans(enabled,next_run_at);

CREATE INDEX user_group_members_user ON user_group_members(user_id);

CREATE INDEX api_keys_owner ON api_keys(user_id);

CREATE INDEX login_throttle_window ON login_throttle(window_started);

CREATE INDEX user_avatars_asset ON user_avatars(sha256);

CREATE INDEX extension_runs_recent ON extension_runs(started_at);

CREATE INDEX music_tracks_library ON music_tracks(library_id,available);

CREATE INDEX work_posters_asset ON work_posters(sha256);

CREATE INDEX library_artwork_asset ON library_artwork(sha256);

CREATE INDEX media_group_members_group_work ON media_group_members(group_id,work_id);

CREATE INDEX work_images_owner ON work_image_entries(work_id,kind,position,id);

CREATE INDEX work_image_data_asset ON work_image_data(sha256);

CREATE INDEX poster_previews_asset ON poster_previews(sha256);

CREATE INDEX inventory_display_sort ON inventory_works(kind,display_title_sort,id);

CREATE INDEX inventory_library_display_sort ON inventory_works(library_id,kind,display_title_sort,id);

CREATE INDEX playlist_shares_user ON playlist_shares(user_id,list_id);

CREATE INDEX library_folders_parent ON library_folders(parent_id, active);

CREATE INDEX extension_avatar_images_pack ON extension_avatar_images(pack_id);

CREATE INDEX extension_avatar_images_asset ON extension_avatar_images(sha256);

CREATE INDEX media_work_external_ids_lookup ON media_work_external_ids(kind, provider, external_id, work_id);

CREATE INDEX media_match_candidates_status ON media_match_candidates(status, score DESC, id);

CREATE INDEX media_merge_history_group ON media_merge_history(group_id, created_at DESC);

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

CREATE INDEX work_image_provider_source ON work_image_entries(work_id,provider,source_url_hash);

CREATE INDEX active_playbacks_session ON active_playbacks(session_hash);

CREATE INDEX active_playbacks_updated ON active_playbacks(updated_at);

CREATE INDEX extension_data_expiry ON extension_data(extension_id,expires_at);

CREATE INDEX extension_objects_owner ON extension_objects(extension_id,scope_kind,scope_id);

CREATE INDEX automation_events_pending ON automation_events(created_at,id) WHERE dispatched_at IS NULL;

CREATE INDEX automation_sweeps_pending ON automation_sweeps(created_at,id) WHERE completed_at IS NULL;

CREATE INDEX durable_tasks_dispatch ON durable_tasks(node_id,status,priority DESC,created_at,id);

CREATE INDEX durable_tasks_expiry ON durable_tasks(status,lease_until);

CREATE INDEX durable_tasks_library ON durable_tasks(library_id,status);

CREATE INDEX durable_tasks_recent ON durable_tasks(created_at DESC,id);

CREATE INDEX durable_tasks_pending ON durable_tasks(status) WHERE status IN ('queued','running','paused');

CREATE INDEX durable_tasks_leased ON durable_tasks(lease_until,id) WHERE lease_until IS NOT NULL;

CREATE INDEX durable_tasks_leased_node ON durable_tasks(node_id) WHERE lease_until IS NOT NULL;

CREATE INDEX durable_tasks_leased_library ON durable_tasks(library_id) WHERE lease_until IS NOT NULL;

CREATE INDEX durable_tasks_ready ON durable_tasks(node_id,available_at) WHERE status='queued' AND lease_until IS NULL;

CREATE INDEX durable_tasks_system_ready ON durable_tasks(priority DESC,created_at,id) WHERE category='system' AND status='queued';

CREATE INDEX automation_tasks_extension ON automation_task_links(extension_id,task_id);

CREATE INDEX automation_tasks_provider ON automation_task_links(provider,task_id);

CREATE INDEX automation_provider_cache_expiry ON automation_provider_cache(provider, expires_at);

CREATE TRIGGER inventory_works_added_at_insert AFTER INSERT ON inventory_works
WHEN NEW.added_at IS NULL BEGIN
  UPDATE inventory_works SET added_at=unixepoch() WHERE id=NEW.id;
END;

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

-- `+has_available_asset` keeps the planner on the primary key: indexing the flag would visit
-- every unavailable work once per inserted asset. Existing databases: asset_availability_insert.sql.

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

CREATE TABLE watch_parties (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    host_user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    media_id TEXT NOT NULL,
    control_mode TEXT NOT NULL DEFAULT 'host_only' CHECK(control_mode IN ('host_only', 'shared', 'request')),
    state TEXT NOT NULL DEFAULT 'waiting' CHECK(state IN ('waiting', 'playing', 'paused', 'ended')),
    current_position_ms INTEGER NOT NULL DEFAULT 0,
    position_updated_at_ms INTEGER NOT NULL DEFAULT 0,
    is_locked INTEGER NOT NULL DEFAULT 0 CHECK(is_locked IN (0, 1)),
    wait_for_buffering INTEGER NOT NULL DEFAULT 1 CHECK(wait_for_buffering IN (0, 1)),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    ended_at INTEGER
) STRICT;

CREATE TABLE watch_party_members (
    party_id TEXT NOT NULL REFERENCES watch_parties(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role TEXT NOT NULL DEFAULT 'participant' CHECK(role IN ('host', 'participant')),
    is_ready INTEGER NOT NULL DEFAULT 0 CHECK(is_ready IN (0, 1)),
    is_buffering INTEGER NOT NULL DEFAULT 0 CHECK(is_buffering IN (0, 1)),
    joined_at INTEGER NOT NULL DEFAULT (unixepoch()),
    last_seen_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY(party_id, user_id)
) STRICT;

CREATE TABLE watch_party_invites (
    id TEXT PRIMARY KEY NOT NULL,
    party_id TEXT NOT NULL REFERENCES watch_parties(id) ON DELETE CASCADE,
    token TEXT NOT NULL UNIQUE,
    created_by TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    target_user_id TEXT REFERENCES users(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    expires_at INTEGER NOT NULL,
    revoked_at INTEGER
) STRICT;

CREATE TABLE watch_party_messages (
    id TEXT PRIMARY KEY NOT NULL,
    party_id TEXT NOT NULL REFERENCES watch_parties(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    content TEXT NOT NULL CHECK(length(content) BETWEEN 1 AND 500),
    kind TEXT NOT NULL DEFAULT 'chat' CHECK(kind IN ('chat', 'system', 'reaction')),
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;

CREATE TABLE watch_party_queue (
    id TEXT PRIMARY KEY NOT NULL,
    party_id TEXT NOT NULL REFERENCES watch_parties(id) ON DELETE CASCADE,
    media_id TEXT NOT NULL,
    added_by TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    position INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;

CREATE TABLE watch_party_polls (
    id TEXT PRIMARY KEY NOT NULL,
    party_id TEXT NOT NULL REFERENCES watch_parties(id) ON DELETE CASCADE,
    question TEXT NOT NULL CHECK(length(question) BETWEEN 1 AND 200),
    options_json TEXT NOT NULL,
    created_by TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    is_closed INTEGER NOT NULL DEFAULT 0 CHECK(is_closed IN (0, 1)),
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;

CREATE TABLE watch_party_poll_votes (
    poll_id TEXT NOT NULL REFERENCES watch_party_polls(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    option_index INTEGER NOT NULL,
    PRIMARY KEY(poll_id, user_id)
) STRICT;

CREATE TABLE schema_version(version INTEGER PRIMARY KEY CHECK(version=1));
INSERT INTO schema_version(version) VALUES(1);

-- Initial rows
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
('clean-activity-log', 'Nettoyer le journal des extensions', 'Supprime au plus 500 événements du journal des extensions antérieurs à la rétention configurée.', 'Maintenance', 1440, 1, strftime('%s','now') + 86400),
('clean-cache', 'Nettoyer le cache et fichiers temporaires', 'Supprime les fichiers temporaires périmés des dossiers appartenant à Aether (jamais les médias).', 'Maintenance', 1440, 1, strftime('%s','now') + 86400),
('clean-playlists', 'Nettoyer les listes et collections', 'Retire des listes de lecture personnelles les éléments dont les médias ont été supprimés.', 'Maintenance', 1440, 1, strftime('%s','now') + 86400);

INSERT INTO inventory_works_unicode_migration (completed_at) VALUES ('1970-01-01 00:00:00');
CREATE TABLE IF NOT EXISTS extension_update_revisions (
 extension_id TEXT PRIMARY KEY REFERENCES extensions(id) ON DELETE CASCADE,
 package_json TEXT NOT NULL, settings_json TEXT NOT NULL,
 enabled BIGINT NOT NULL, all_libraries BIGINT NOT NULL,
 target_sha256 TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS extension_update_data AS SELECT * FROM extension_data WHERE 1=0;
CREATE INDEX IF NOT EXISTS extension_update_data_id ON extension_update_data(extension_id);
CREATE TABLE IF NOT EXISTS extension_update_objects AS SELECT * FROM extension_objects WHERE 1=0;
CREATE INDEX IF NOT EXISTS extension_update_objects_id ON extension_update_objects(extension_id);
CREATE TABLE IF NOT EXISTS extension_update_chunks AS SELECT * FROM extension_object_chunks WHERE 1=0;
CREATE INDEX IF NOT EXISTS extension_update_chunks_id ON extension_update_chunks(object_id);
CREATE TABLE IF NOT EXISTS extension_update_namespaces AS SELECT * FROM extension_data_namespaces WHERE 1=0;
CREATE INDEX IF NOT EXISTS extension_update_namespaces_id ON extension_update_namespaces(extension_id);
CREATE TABLE IF NOT EXISTS extension_update_grants AS SELECT * FROM extension_grants WHERE 1=0;
CREATE INDEX IF NOT EXISTS extension_update_grants_id ON extension_update_grants(extension_id);


-- ===== Features added after the first schema draft =====


-- ----- extension_run -----
CREATE TABLE extension_run_owners (
 run_id TEXT PRIMARY KEY REFERENCES extension_runs(id) ON DELETE CASCADE,
 owner_id TEXT NOT NULL, fence_token TEXT NOT NULL,
 lease_until BIGINT NOT NULL
);
CREATE INDEX extension_run_owner_expiry ON extension_run_owners(lease_until);

-- ----- subtitle -----
-- Additive, idempotent schema: no catalogue rebuild or media writes.
CREATE TABLE subtitle_searches (
 id TEXT PRIMARY KEY, actor_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
 asset_id TEXT NOT NULL REFERENCES inventory_assets(id) ON DELETE CASCADE,
 provider_id TEXT NOT NULL, generation TEXT NOT NULL, fingerprint TEXT NOT NULL,
 result_json TEXT NOT NULL, expires_at BIGINT NOT NULL
);
CREATE INDEX subtitle_search_expiry ON subtitle_searches(expires_at);
CREATE TABLE subtitle_downloads (
 id TEXT PRIMARY KEY, asset_id TEXT NOT NULL REFERENCES inventory_assets(id) ON DELETE CASCADE,
 provider_id TEXT NOT NULL, file_id TEXT NOT NULL, fingerprint TEXT NOT NULL,
 track_json TEXT NOT NULL, status TEXT NOT NULL CHECK(status IN ('pending','completed','failed')),
 error_code TEXT, created_at BIGINT NOT NULL, content TEXT,
 UNIQUE(asset_id,provider_id,file_id)
);
CREATE INDEX subtitle_download_asset ON subtitle_downloads(asset_id,status);
CREATE TABLE subtitle_provider_limits (
 provider_id TEXT PRIMARY KEY, next_allowed_at BIGINT NOT NULL
);
CREATE TABLE subtitle_download_limits (
 provider_id TEXT PRIMARY KEY, remaining BIGINT, reset_time_utc TEXT,
 next_allowed_at BIGINT NOT NULL
);

-- ----- recommendation -----
CREATE TABLE recommendation_preferences (
 user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
 enabled BIGINT NOT NULL DEFAULT 1 CHECK(enabled IN (0,1)),
 exclude_watched BIGINT NOT NULL DEFAULT 1 CHECK(exclude_watched IN (0,1)),
 history_days BIGINT NOT NULL DEFAULT 180 CHECK(history_days BETWEEN 1 AND 3650)
);

-- ----- notification -----
-- Additive notification outbox; contains no filesystem/media paths.
CREATE TABLE notification_channels (
 id TEXT PRIMARY KEY, name TEXT NOT NULL, recipient_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
 kind TEXT NOT NULL CHECK(kind IN ('email','ntfy','gotify','webhook')), enabled BIGINT NOT NULL,
 config_json TEXT NOT NULL, generation TEXT NOT NULL, created_at BIGINT NOT NULL,
 next_allowed_at BIGINT NOT NULL DEFAULT 0
);
CREATE TABLE notification_preferences (
 user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
 enabled BIGINT NOT NULL DEFAULT 0, scans BIGINT NOT NULL DEFAULT 0, tasks BIGINT NOT NULL DEFAULT 0
);
-- Separate additive subscription preserves databases created before content events.
CREATE TABLE notification_content_preferences (
 user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
 enabled BIGINT NOT NULL DEFAULT 0, since_at BIGINT NOT NULL
);
CREATE TABLE application_notification_preferences (
 user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
 kind TEXT NOT NULL CHECK(kind IN ('requests','reports')), enabled BIGINT NOT NULL DEFAULT 0,
 since_at BIGINT NOT NULL, PRIMARY KEY(user_id,kind)
);
CREATE TABLE notification_events (
 id TEXT PRIMARY KEY, kind TEXT NOT NULL, task_id TEXT NOT NULL, library_id TEXT,
 occurred_at BIGINT NOT NULL, source_attempt BIGINT NOT NULL, source_status TEXT NOT NULL
);
CREATE INDEX notification_event_retention ON notification_events(occurred_at);
CREATE TABLE notification_deliveries (
 id TEXT PRIMARY KEY, event_id TEXT NOT NULL REFERENCES notification_events(id) ON DELETE CASCADE,
 channel_id TEXT NOT NULL REFERENCES notification_channels(id) ON DELETE CASCADE,
 generation TEXT NOT NULL, status TEXT NOT NULL CHECK(status IN ('pending','sending','sent','failed','suppressed')),
 attempt BIGINT NOT NULL DEFAULT 0, available_at BIGINT NOT NULL, lease_until BIGINT,
 lease_token TEXT, error_code TEXT, completed_at BIGINT, UNIQUE(event_id,channel_id)
);
CREATE INDEX notification_delivery_due ON notification_deliveries(status,available_at);

-- ----- statistics -----
-- Additive, shared SQLite/PostgreSQL schema. No media paths, IPs or credentials.
CREATE TABLE statistics_settings (
 singleton BIGINT PRIMARY KEY CHECK(singleton=1), enabled BIGINT NOT NULL DEFAULT 0,
 event_retention_days BIGINT NOT NULL DEFAULT 365, summary_retention_days BIGINT NOT NULL DEFAULT 0,
 first_enabled_at BIGINT, generation BIGINT NOT NULL DEFAULT 0
);
CREATE TABLE statistics_collection_periods (
 id TEXT PRIMARY KEY, started_at BIGINT NOT NULL, ended_at BIGINT
);
CREATE TABLE statistics_sessions (
 id TEXT PRIMARY KEY, user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
 owner_hash TEXT NOT NULL, generation BIGINT NOT NULL,
 media_id TEXT NOT NULL, canonical_id TEXT NOT NULL, asset_id TEXT NOT NULL,
 library_id TEXT NOT NULL, node_id TEXT NOT NULL, kind TEXT NOT NULL,
 title TEXT NOT NULL, artist TEXT NOT NULL, album TEXT NOT NULL, device TEXT NOT NULL,
 started_at BIGINT NOT NULL, updated_at BIGINT NOT NULL, ended_at BIGINT,
 state_json TEXT NOT NULL, seconds DOUBLE PRECISION NOT NULL DEFAULT 0,
 covered_seconds DOUBLE PRECISION NOT NULL DEFAULT 0, duration DOUBLE PRECISION NOT NULL DEFAULT 0,
 completed BIGINT NOT NULL DEFAULT 0, last_sequence BIGINT NOT NULL DEFAULT 0,
 end_reason TEXT NOT NULL DEFAULT ''
);
CREATE INDEX statistics_sessions_date ON statistics_sessions(started_at,id);
CREATE INDEX statistics_sessions_user_date ON statistics_sessions(user_id,started_at,id);
CREATE INDEX statistics_sessions_active ON statistics_sessions(ended_at,updated_at);
CREATE TABLE statistics_events (
 session_id TEXT NOT NULL REFERENCES statistics_sessions(id) ON DELETE CASCADE,
 sequence BIGINT NOT NULL, occurred_at BIGINT NOT NULL, received_at BIGINT NOT NULL,
 event_json TEXT NOT NULL, PRIMARY KEY(session_id,sequence)
);
CREATE INDEX statistics_events_retention ON statistics_events(received_at);
CREATE TABLE statistics_daily (
 session_id TEXT NOT NULL REFERENCES statistics_sessions(id) ON DELETE CASCADE,
 day BIGINT NOT NULL, seconds DOUBLE PRECISION NOT NULL DEFAULT 0,
 PRIMARY KEY(session_id,day)
);
CREATE TABLE statistics_dimensions (
 session_id TEXT NOT NULL REFERENCES statistics_sessions(id) ON DELETE CASCADE,
 dimension TEXT NOT NULL, label TEXT NOT NULL, seconds DOUBLE PRECISION NOT NULL DEFAULT 0,
 PRIMARY KEY(session_id,dimension,label)
);
-- Yearly recap visibility (JellyTrack wrappedVisible/period). A separate singleton
-- keeps this additive and idempotent on both engines without ALTER TABLE.
CREATE TABLE statistics_wrapped_settings (
 singleton BIGINT PRIMARY KEY CHECK(singleton=1), visible BIGINT NOT NULL DEFAULT 1,
 period_enabled BIGINT NOT NULL DEFAULT 0,
 start_month BIGINT NOT NULL DEFAULT 12, start_day BIGINT NOT NULL DEFAULT 1,
 end_month BIGINT NOT NULL DEFAULT 1, end_day BIGINT NOT NULL DEFAULT 31
);
-- Personal visibility is separate from collection, disabled until explicitly allowed.
CREATE TABLE statistics_visibility (
 singleton BIGINT PRIMARY KEY CHECK(singleton=1), users_can_view BIGINT NOT NULL DEFAULT 0 CHECK(users_can_view IN (0,1))
);

-- ----- acquisition -----
CREATE TABLE acquisition_settings (
 singleton BIGINT PRIMARY KEY CHECK(singleton=1), auto_approve BIGINT NOT NULL DEFAULT 0 CHECK(auto_approve IN (0,1)), max_pending_per_user BIGINT NOT NULL DEFAULT 20
);
CREATE TABLE acquisition_servers (
 id TEXT PRIMARY KEY, name TEXT NOT NULL, kind TEXT NOT NULL CHECK(kind IN ('radarr','sonarr','lidarr')),
 base_url TEXT NOT NULL, api_key TEXT NOT NULL, allowed_ips_json TEXT NOT NULL,
 library_id TEXT NOT NULL REFERENCES libraries(id), root_folder TEXT NOT NULL, quality_profile_id BIGINT NOT NULL,
 metadata_profile_id BIGINT, is_default BIGINT NOT NULL DEFAULT 0 CHECK(is_default IN (0,1)), enabled BIGINT NOT NULL DEFAULT 1 CHECK(enabled IN (0,1)), generation TEXT NOT NULL
);
CREATE TABLE media_requests (
 id TEXT PRIMARY KEY, user_id TEXT REFERENCES users(id) ON DELETE SET NULL, server_id TEXT REFERENCES acquisition_servers(id),
 library_id TEXT REFERENCES libraries(id), kind TEXT NOT NULL, title TEXT NOT NULL, external_id TEXT NOT NULL, external_provider TEXT NOT NULL,
 input_json TEXT NOT NULL, status TEXT NOT NULL, error_code TEXT, remote_id TEXT, work_id TEXT,
 created_at BIGINT NOT NULL, updated_at BIGINT NOT NULL, imported BIGINT NOT NULL DEFAULT 0 CHECK(imported IN (0,1)),
 source_key TEXT UNIQUE, author_label TEXT, source_status TEXT, source_remote_id TEXT
);
CREATE INDEX media_requests_owner ON media_requests(user_id,created_at);
CREATE INDEX media_requests_identity ON media_requests(server_id,kind,external_id,status);
CREATE TABLE acquisition_jobs (
 request_id TEXT PRIMARY KEY REFERENCES media_requests(id) ON DELETE CASCADE, available_at BIGINT NOT NULL,
 lease_token TEXT, lease_until BIGINT NOT NULL DEFAULT 0, attempts BIGINT NOT NULL DEFAULT 0, phase TEXT NOT NULL DEFAULT 'initial'
);
CREATE TABLE acquisition_events (
 id TEXT PRIMARY KEY, request_id TEXT NOT NULL REFERENCES media_requests(id) ON DELETE CASCADE,
 actor_id TEXT, status TEXT NOT NULL, code TEXT, created_at BIGINT NOT NULL
);

-- ----- reports -----
CREATE TABLE media_reports (
 id TEXT PRIMARY KEY, user_id TEXT REFERENCES users(id) ON DELETE SET NULL, library_id TEXT REFERENCES libraries(id) ON DELETE SET NULL,
 work_id TEXT, track_id TEXT, asset_id TEXT, category TEXT NOT NULL, description TEXT NOT NULL,
 status TEXT NOT NULL, position_seconds DOUBLE PRECISION, end_seconds DOUBLE PRECISION, context_json TEXT NOT NULL DEFAULT '{}',
 created_at BIGINT NOT NULL, updated_at BIGINT NOT NULL, source_key TEXT UNIQUE, author_label TEXT
);
CREATE INDEX media_reports_owner ON media_reports(user_id,updated_at);
CREATE TABLE media_report_comments (
 id TEXT PRIMARY KEY, report_id TEXT NOT NULL REFERENCES media_reports(id) ON DELETE CASCADE,
 user_id TEXT REFERENCES users(id) ON DELETE SET NULL, author_label TEXT, message TEXT NOT NULL, created_at BIGINT NOT NULL, source_key TEXT UNIQUE
);
CREATE TABLE media_report_events (
 id TEXT PRIMARY KEY, report_id TEXT NOT NULL REFERENCES media_reports(id) ON DELETE CASCADE,
 user_id TEXT, kind TEXT NOT NULL, status TEXT NOT NULL, created_at BIGINT NOT NULL
);
CREATE TABLE application_imports (
 id TEXT PRIMARY KEY, source_url TEXT NOT NULL, connection_json TEXT NOT NULL, status TEXT NOT NULL,
 phase TEXT NOT NULL, page_offset BIGINT NOT NULL DEFAULT 0, processed BIGINT NOT NULL DEFAULT 0,
 errors BIGINT NOT NULL DEFAULT 0, mappings_json TEXT NOT NULL DEFAULT '{}', preview_json TEXT NOT NULL,
 lease_token TEXT, lease_until BIGINT NOT NULL DEFAULT 0, updated_at BIGINT NOT NULL, last_error TEXT
);

-- ----- report_triage -----
-- Administrator triage of media reports: priority proposed by the Signalements
-- extension at creation (reports.classify), adjustable by an administrator.
-- Suggested checks are advisory codes; nothing here schedules a repair.
CREATE TABLE media_report_triage (
 report_id TEXT PRIMARY KEY REFERENCES media_reports(id) ON DELETE CASCADE,
 priority TEXT NOT NULL CHECK(priority IN ('high','normal','low')),
 code TEXT, checks_json TEXT NOT NULL DEFAULT '[]', updated_at BIGINT NOT NULL
);
CREATE INDEX media_reports_queue ON media_reports(status,updated_at);
CREATE INDEX media_requests_queue ON media_requests(status,created_at);

-- ----- person_images -----
-- Additive, idempotent cache of cast/crew portraits fetched from metadata providers.
-- Only the SHA-256 of the provider URL is stored; bytes live in the asset store.
CREATE TABLE person_images (
 source_hash TEXT PRIMARY KEY,
 sha256 TEXT REFERENCES assets(sha256),
 width BIGINT,
 height BIGINT,
 error_code TEXT,
 fetched_at BIGINT NOT NULL
);
CREATE INDEX person_images_asset ON person_images(sha256);

-- ----- metadata_translations -----
-- Additive, idempotent store of translated texts for works (films, series, episodes) and
-- seasons. Invariant facts (year, runtime, identifiers, original language) stay on
-- inventory_works. One row per (entity, locale, source): a manual correction, an NFO and a
-- provider translation coexist and are ranked at read time, so a refresh never overwrites
-- a hand-written value. Works with no row here keep their legacy columns and JSON.
CREATE TABLE work_translations (
 work_id TEXT NOT NULL REFERENCES inventory_works(id) ON DELETE CASCADE,
 locale TEXT NOT NULL,
 language TEXT NOT NULL,
 source TEXT NOT NULL,
 priority BIGINT NOT NULL,
 title TEXT,
 sort_title TEXT,
 overview TEXT,
 tagline TEXT,
 provider_locale TEXT,
 updated_at BIGINT NOT NULL,
 PRIMARY KEY(work_id, locale, source)
);
CREATE INDEX work_translations_language ON work_translations(work_id, language);
CREATE TABLE season_translations (
 season_id TEXT NOT NULL REFERENCES inventory_seasons(id) ON DELETE CASCADE,
 locale TEXT NOT NULL,
 language TEXT NOT NULL,
 source TEXT NOT NULL,
 priority BIGINT NOT NULL,
 title TEXT,
 sort_title TEXT,
 overview TEXT,
 tagline TEXT,
 provider_locale TEXT,
 updated_at BIGINT NOT NULL,
 PRIMARY KEY(season_id, locale, source)
);
CREATE INDEX season_translations_language ON season_translations(season_id, language);
-- Searching by any translated or original title without duplicating works: one row per distinct
-- folded title, rebuilt for a work whenever its translations or metadata change (see
-- translations/search.rs) and filled once for existing databases by a journaled Rust step.
CREATE TABLE work_search_titles (
 work_id TEXT NOT NULL REFERENCES inventory_works(id) ON DELETE CASCADE,
 title_search TEXT NOT NULL,
 PRIMARY KEY(work_id, title_search)
);
-- Lets the catalogue ask "does anyone have a translation in this language?" without a table scan.
CREATE INDEX work_translations_by_language ON work_translations(language, work_id);

-- ----- quick_connect -----
CREATE TABLE device_connections (
    code_hash TEXT PRIMARY KEY,
    requester_hash TEXT NOT NULL UNIQUE,
    device_label TEXT NOT NULL,
    approver_session_hash TEXT REFERENCES sessions(token_hash) ON DELETE CASCADE,
    expires_at BIGINT NOT NULL
);
CREATE INDEX device_connections_expiry ON device_connections(expires_at);

-- ----- task_runs -----
-- Fences for durable non-scan tasks (analysis, preparation, images, trickplay).
-- They are tracked per run, never through the library scan status, so a
-- post-scan preview cannot rewrite a completed scan summary or fence it out.
CREATE TABLE active_task_runs (
 run_id TEXT PRIMARY KEY NOT NULL,
 library_id TEXT NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
 node_id TEXT NOT NULL,
 status TEXT NOT NULL DEFAULT 'running' CHECK(status IN ('running','completed','error')),
 error TEXT NOT NULL DEFAULT ''
);
CREATE INDEX active_task_runs_library ON active_task_runs(library_id);

-- ----- task_triggers -----
-- Jellyfin-style task triggers shared by system tasks, library scans and the backup.
-- Additive and idempotent: legacy interval rows are converted once per task, and the legacy
-- `next_due_at` / `next_run_at` columns remain the computed "earliest next run" cache.
CREATE TABLE task_triggers (
 id TEXT PRIMARY KEY,
 task_kind TEXT NOT NULL CHECK(task_kind IN ('system','scan','backup')),
 task_id TEXT NOT NULL,
 trigger_type TEXT NOT NULL CHECK(trigger_type IN ('daily','weekly','interval','startup')),
 time_of_day_minutes BIGINT CHECK(time_of_day_minutes IS NULL OR time_of_day_minutes BETWEEN 0 AND 1439),
 day_of_week BIGINT CHECK(day_of_week IS NULL OR day_of_week BETWEEN 0 AND 6),
 interval_minutes BIGINT CHECK(interval_minutes IS NULL OR interval_minutes BETWEEN 1 AND 10080),
 max_runtime_seconds BIGINT CHECK(max_runtime_seconds IS NULL OR max_runtime_seconds BETWEEN 60 AND 604800),
 enabled BIGINT NOT NULL DEFAULT 1 CHECK(enabled IN (0,1)),
 next_fire_at BIGINT,
 created_at BIGINT NOT NULL
);
CREATE INDEX task_triggers_task ON task_triggers(task_kind,task_id);
-- One row per task once its legacy interval has been converted (never converted twice).
CREATE TABLE task_trigger_migrations (
 task_kind TEXT NOT NULL,
 task_id TEXT NOT NULL,
 migrated_at BIGINT NOT NULL,
 PRIMARY KEY(task_kind,task_id)
);

-- ----- artwork_webp -----
-- Additive metadata for application, legacy and inventoried local artwork.
-- image_id deliberately has no FK: local images and legacy posters have distinct owners.
CREATE TABLE library_artwork_previews (
    library_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    width BIGINT NOT NULL,
    sha256 TEXT NOT NULL REFERENCES assets(sha256),
    PRIMARY KEY(library_id,kind,width),
    FOREIGN KEY(library_id,kind) REFERENCES library_artwork(library_id,kind) ON DELETE CASCADE
);
CREATE INDEX library_artwork_previews_asset ON library_artwork_previews(sha256);
CREATE TABLE artwork_annotations (
    work_id TEXT NOT NULL REFERENCES inventory_works(id) ON DELETE CASCADE,
    image_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    pinned BIGINT NOT NULL DEFAULT 0 CHECK(pinned IN (0,1)),
    source_sha256 TEXT,
    encoding TEXT,
    release_evidence TEXT,
    PRIMARY KEY(work_id,image_id)
);
CREATE INDEX artwork_source_digest ON artwork_annotations(source_sha256,kind,encoding);
CREATE UNIQUE INDEX artwork_one_pin ON artwork_annotations(work_id,kind) WHERE pinned=1;
CREATE TABLE image_migration_attempts (
    sha256 TEXT PRIMARY KEY,
    attempts BIGINT NOT NULL DEFAULT 0,
    retry_at BIGINT NOT NULL DEFAULT 0,
    outcome TEXT NOT NULL DEFAULT 'pending'
);
CREATE TABLE artwork_collection_jobs (
    id TEXT PRIMARY KEY,
    library_id TEXT NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    owner TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    parameters TEXT NOT NULL,
    cursor_work TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'queued' CHECK(status IN ('queued','running','completed','paused','cancelled')),
    lease_token TEXT NOT NULL DEFAULT '',
    lease_until BIGINT NOT NULL DEFAULT 0,
    next_run_at BIGINT NOT NULL DEFAULT 0,
    processed BIGINT NOT NULL DEFAULT 0,
    downloaded BIGINT NOT NULL DEFAULT 0,
    duplicates BIGINT NOT NULL DEFAULT 0,
    skipped BIGINT NOT NULL DEFAULT 0,
    errors BIGINT NOT NULL DEFAULT 0,
    work_attempts BIGINT NOT NULL DEFAULT 0,
    last_error TEXT NOT NULL DEFAULT '',
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);
CREATE UNIQUE INDEX artwork_one_collection ON artwork_collection_jobs(library_id) WHERE status IN ('queued','running','paused');

-- ----- artwork_auto -----
-- Automatic artwork: one row per work the background filler has already handled in
-- a given mode ('single' = one image of each enabled kind, 'pool' = multi-image pool),
-- so a library pass never asks the providers twice for the same work.
CREATE TABLE artwork_auto_attempts (
 work_id TEXT NOT NULL,
 mode TEXT NOT NULL,
 attempted_at BIGINT NOT NULL,
 PRIMARY KEY(work_id, mode)
);

-- ----- library_media_tasks -----
-- Per-library media sub-tasks of the scheduler (previews, intros and credits, images).
-- One row per library and sub-task: when its last run was requested and by what (an
-- administrator or a scan). The work itself stays in `durable_tasks` and
-- `artwork_collection_jobs`; this only dates the run the scheduler reports.
CREATE TABLE library_media_task_runs (
 library_id TEXT NOT NULL,
 task TEXT NOT NULL CHECK(task IN ('trickplay','intros','artwork')),
 started_at BIGINT NOT NULL,
 origin TEXT NOT NULL DEFAULT 'manual' CHECK(origin IN ('manual','scan')),
 PRIMARY KEY(library_id, task)
);

-- ----- playback_grants -----
CREATE TABLE playback_grants (
    token_hash TEXT PRIMARY KEY,
    session_hash TEXT NOT NULL REFERENCES sessions(token_hash) ON DELETE CASCADE,
    asset_id TEXT NOT NULL,
    expires_at BIGINT NOT NULL
);
CREATE INDEX playback_grants_session ON playback_grants(session_hash);
CREATE INDEX playback_grants_expiry ON playback_grants(expires_at);

-- ----- account_profiles -----
-- "Who's watching?" profiles: each profile is a full user without credentials,
-- attached to the account that created it. The owner may have its own row
-- (user_id = owner_id) only to hold a PIN. Profiles inherit the owner's
-- library access through effective_library_permissions (role_groups.sql).
CREATE TABLE account_profiles (
 user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
 owner_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
 pin_hash TEXT,
 created_at BIGINT NOT NULL
);
CREATE INDEX account_profiles_owner ON account_profiles(owner_id);

-- ----- request_policies -----
-- Who may request what, what is approved without an administrator, and request quotas.
-- Rules are small JSON documents validated by Core on write (acquisition/policy.rs).
-- A missing row inherits: account rule > group rules > server default > built-in default.
-- Rules grant no role and no library access; they only shape the requests application.
CREATE TABLE request_default_policy (
 singleton BIGINT PRIMARY KEY CHECK(singleton=1), policy_json TEXT NOT NULL
);
CREATE TABLE request_group_policies (
 group_id TEXT PRIMARY KEY REFERENCES user_groups(id) ON DELETE CASCADE, policy_json TEXT NOT NULL, updated_at BIGINT NOT NULL
);
CREATE TABLE request_user_policies (
 user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE, policy_json TEXT NOT NULL, updated_at BIGINT NOT NULL
);
-- Quota windows count one account's requests by date.
CREATE INDEX media_requests_quota ON media_requests(user_id,kind,created_at);

-- ----- oidc_app -----
-- Single sign-on started by a native application in the system browser.
-- A row is the pending request (bound to the OIDC state), then the one-time
-- code handed back to the application, redeemed with its verifier.
CREATE TABLE oidc_app_grants (
    state_hash TEXT PRIMARY KEY,
    challenge TEXT NOT NULL,
    redirect TEXT NOT NULL,
    code_hash TEXT UNIQUE,
    user_id TEXT REFERENCES users(id) ON DELETE CASCADE,
    expires_at BIGINT NOT NULL
);
CREATE INDEX oidc_app_grants_expiry ON oidc_app_grants(expires_at);

-- ----- media_kinds -----
-- Media types denied to a user or to every member of a group (films, series,
-- music). The most restrictive rule wins; library grants stay unchanged.
CREATE TABLE user_media_kind_denials (
 user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
 kind TEXT NOT NULL CHECK(kind IN ('films','series','music')),
 PRIMARY KEY(user_id,kind)
);
CREATE TABLE group_media_kind_denials (
 group_id TEXT NOT NULL REFERENCES user_groups(id) ON DELETE CASCADE,
 kind TEXT NOT NULL CHECK(kind IN ('films','series','music')),
 PRIMARY KEY(group_id,kind)
);

-- ----- role_groups -----
CREATE VIEW effective_user_group_members AS
SELECT group_id,user_id FROM user_group_members
WHERE group_id NOT IN ('builtin-role-user','builtin-role-administrator')
UNION SELECT 'builtin-role-user',id FROM users WHERE role='user'
UNION SELECT 'builtin-role-administrator',id FROM users WHERE role='administrator';
-- Every user-facing query reads this view: denied media types never leak
-- through catalogue, search, details, playback, music, lists or SyncPlay.
CREATE VIEW effective_library_permissions AS
WITH account_grants AS (
    SELECT library_id,user_id FROM library_permissions
    UNION SELECT l.library_id,m.user_id FROM user_group_libraries l JOIN effective_user_group_members m ON m.group_id=l.group_id
    UNION SELECT l.library_id,u.id FROM user_group_libraries l JOIN user_groups g ON g.id=l.group_id CROSS JOIN users u WHERE g.is_default=1
), direct_grants AS (
    SELECT library_id,user_id FROM account_grants
    -- A profile sees what its owner sees; an administrator's access is implicit, so
    -- its profiles get every library administrators have not been excluded from.
    UNION SELECT a.library_id,p.user_id FROM account_profiles p JOIN account_grants a ON a.user_id=p.owner_id WHERE p.user_id<>p.owner_id
    UNION SELECT lib.id,p.user_id FROM account_profiles p JOIN users o ON o.id=p.owner_id AND o.role='administrator' CROSS JOIN libraries lib
      WHERE p.user_id<>p.owner_id AND COALESCE(lib.options_json,'') NOT LIKE '%adminAccess__false%'
), granted AS (
    SELECT d.library_id,d.user_id FROM direct_grants d WHERE NOT EXISTS(SELECT 1 FROM library_folders f WHERE f.library_id=d.library_id)
    UNION SELECT f.library_id,d.user_id FROM direct_grants d JOIN library_folders f ON f.parent_id=d.library_id AND f.active=1
)
SELECT g.library_id,g.user_id FROM granted g JOIN libraries lib ON lib.id=g.library_id
WHERE NOT EXISTS(SELECT 1 FROM user_media_kind_denials x WHERE x.user_id=g.user_id AND x.kind=lib.kind)
AND NOT EXISTS(SELECT 1 FROM group_media_kind_denials x JOIN effective_user_group_members m ON m.group_id=x.group_id WHERE m.user_id=g.user_id AND x.kind=lib.kind)
AND NOT EXISTS(SELECT 1 FROM group_media_kind_denials x JOIN user_groups dg ON dg.id=x.group_id WHERE dg.is_default=1 AND x.kind=lib.kind);

-- ----- default_avatars -----
-- Separate from the pack: removing a default collection is a durable choice.
CREATE TABLE avatar_gallery_initialization (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1)
);
-- Each built-in collection shipped with the server is offered once, on new and existing
-- installations alike; removing it afterwards is just as durable.
CREATE TABLE avatar_builtin_seeds (
    pack_id TEXT PRIMARY KEY NOT NULL
);

-- ----- avatar_requirements -----
CREATE TABLE avatar_requirements (
 singleton INTEGER PRIMARY KEY CHECK(singleton=1),
 required INTEGER NOT NULL CHECK(required IN (0,1))
);

-- ----- library_deletions -----
-- Libraries removed by an administrator. Media files and history are kept,
-- but the library and its folders are disabled and hidden everywhere.
CREATE TABLE deleted_libraries (
 library_id TEXT PRIMARY KEY REFERENCES libraries(id) ON DELETE CASCADE,
 deleted_at BIGINT NOT NULL
);

-- ----- content_fingerprints -----
-- Partial content fingerprints (size + sampled BLAKE3, see crates/node/src/fingerprint.rs).
-- A hint used only to recognise a file that was renamed or moved inside one library so its
-- work, manual corrections, translations and prepared caches follow it. Never an identity:
-- rows carry no authority beyond "candidate", and losing the whole table only loses the hint.
-- Additive and idempotent on SQLite and PostgreSQL; old Nodes and old rows keep working.
CREATE TABLE inventory_asset_fingerprints (
 asset_id TEXT PRIMARY KEY REFERENCES inventory_assets(id) ON DELETE CASCADE,
 library_id TEXT NOT NULL,
 size BIGINT NOT NULL,
 modified_ns TEXT NOT NULL,
 fingerprint TEXT NOT NULL
);
CREATE INDEX inventory_asset_fingerprints_lookup ON inventory_asset_fingerprints(library_id, size, fingerprint);
-- Fingerprints reported by a Node for the staged scan, consumed and deleted by its publication.
CREATE TABLE scan_staging_fingerprints (
 scan_id TEXT NOT NULL,
 relative_path TEXT NOT NULL,
 library_id TEXT NOT NULL,
 fingerprint TEXT NOT NULL,
 PRIMARY KEY(scan_id, relative_path)
);

-- ----- account_mfa -----
-- Second factor (TOTP + single-use recovery codes) and passkeys (WebAuthn) for local accounts.
-- Additive and idempotent on SQLite and PostgreSQL. Secrets that only need comparing are hashed;
-- the TOTP seed must stay readable to compute codes, so archives carrying it are encrypted (MDBK0003).
CREATE TABLE user_totp (
 user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
 secret TEXT NOT NULL,
 confirmed_at BIGINT,
 last_step BIGINT NOT NULL DEFAULT 0,
 created_at BIGINT NOT NULL
);
CREATE TABLE user_recovery_codes (
 user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
 code_hash TEXT NOT NULL,
 PRIMARY KEY(user_id, code_hash)
);
CREATE TABLE user_passkeys (
 credential_id TEXT PRIMARY KEY,
 user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
 name TEXT NOT NULL,
 algorithm BIGINT NOT NULL,
 public_key TEXT NOT NULL,
 sign_count BIGINT NOT NULL DEFAULT 0,
 created_at BIGINT NOT NULL,
 last_used_at BIGINT
);
CREATE INDEX user_passkeys_user ON user_passkeys(user_id);
-- Short-lived, single-use proofs: a password accepted but awaiting its second factor ('mfa'),
-- or a WebAuthn challenge ('passkey_register', 'passkey_login'). Only hashes are stored.
CREATE TABLE auth_challenges (
 challenge_hash TEXT PRIMARY KEY,
 purpose TEXT NOT NULL,
 user_id TEXT REFERENCES users(id) ON DELETE CASCADE,
 webauthn_hash TEXT,
 bearer BIGINT NOT NULL DEFAULT 0,
 attempts BIGINT NOT NULL DEFAULT 0,
 expires_at BIGINT NOT NULL
);
CREATE INDEX auth_challenges_expiry ON auth_challenges(expires_at);

-- ----- inventory_assets availability trigger (PostgreSQL maintains it in Rust) -----
-- SQLite only, applied once (versioned step 0001_asset_availability_insert).
-- Same trigger as schema.sql: `+has_available_asset` keeps the planner on the primary key.
-- Without it, inventory_works_avail_sort was chosen and every inserted asset visited every
-- unavailable work, so publishing slowed down with offline libraries and removed files.
CREATE TRIGGER inventory_assets_avail_insert AFTER INSERT ON inventory_assets
WHEN NEW.available = 1 BEGIN
    UPDATE inventory_works SET has_available_asset = 1
    WHERE id IN (
        SELECT e.work_id FROM inventory_sources s
        JOIN inventory_versions v ON v.id = s.version_id
        JOIN inventory_editions e ON e.id = v.edition_id
        WHERE s.id = NEW.source_id
    ) AND +has_available_asset = 0;

    UPDATE inventory_works SET has_available_asset = 1
    WHERE id IN (
        SELECT se.series_id FROM inventory_sources s
        JOIN inventory_versions v ON v.id = s.version_id
        JOIN inventory_editions e ON e.id = v.edition_id
        JOIN inventory_episodes ep ON ep.work_id = e.work_id
        JOIN inventory_seasons se ON se.id = ep.season_id
        WHERE s.id = NEW.source_id
    ) AND +has_available_asset = 0;
END;
