-- Aether Core schema (postgres). Applied once, by `database::initialize_fresh_schema`, to an empty
-- database. There is no migration chain: a change to the schema edits this file and databases
-- created before it are recreated.


-- Fresh PostgreSQL schema. Do not apply to an existing database.

CREATE FUNCTION instr(source_text text, search_text text) RETURNS integer
    LANGUAGE sql IMMUTABLE
    AS $$
    SELECT COALESCE(STRPOS(source_text, search_text), 0);
$$;



CREATE FUNCTION json_extract(doc text, path text) RETURNS text
    LANGUAGE plpgsql IMMUTABLE
    AS $_$
BEGIN
    RETURN doc::jsonb ->> trim(leading '$.' from path);
EXCEPTION WHEN OTHERS THEN
    RETURN NULL;
END;
$_$;



CREATE FUNCTION json_valid(doc text) RETURNS integer
    LANGUAGE plpgsql IMMUTABLE
    AS $$
BEGIN
    PERFORM doc::jsonb;
    RETURN 1;
EXCEPTION WHEN OTHERS THEN
    RETURN 0;
END;
$$;



CREATE FUNCTION unixepoch() RETURNS bigint
    LANGUAGE sql STABLE
    AS $$
    SELECT EXTRACT(EPOCH FROM CURRENT_TIMESTAMP)::bigint;
$$;





CREATE TABLE active_playbacks (
    id text NOT NULL,
    session_hash text NOT NULL,
    work_id text NOT NULL,
    position_seconds double precision NOT NULL,
    duration_seconds double precision NOT NULL,
    paused integer NOT NULL,
    updated_at bigint NOT NULL,
    CONSTRAINT active_playbacks_paused_check CHECK ((paused = ANY (ARRAY[0, 1])))
);



CREATE TABLE active_scan_runs (
    library_id text NOT NULL,
    scan_id text NOT NULL,
    node_id text NOT NULL
);



CREATE TABLE api_key_libraries (
    key_id text NOT NULL,
    library_id text NOT NULL
);



CREATE TABLE api_keys (
    id text NOT NULL,
    name text NOT NULL,
    user_id text NOT NULL,
    token_hash text NOT NULL,
    created_at bigint NOT NULL,
    expires_at bigint NOT NULL,
    revoked_at bigint,
    scope text NOT NULL,
    CONSTRAINT api_keys_name_check CHECK (((length(name) >= 1) AND (length(name) <= 80))),
    CONSTRAINT api_keys_scope_check CHECK ((scope = 'catalog:read'::text))
);



CREATE TABLE artwork_image_sources (
    work_id text NOT NULL,
    source_url_hash text NOT NULL,
    image_id text NOT NULL
);



CREATE TABLE assets (
    sha256 text NOT NULL,
    size bigint NOT NULL,
    mime text NOT NULL,
    created_at bigint NOT NULL,
    orphaned_at bigint,
    CONSTRAINT assets_mime_check CHECK (((length(mime) >= 3) AND (length(mime) <= 100))),
    CONSTRAINT assets_sha256_check CHECK ((length(sha256) = 64)),
    CONSTRAINT assets_size_check CHECK ((size > 0))
);



CREATE TABLE automatic_metadata_provenance (
    work_id text NOT NULL,
    field_name text NOT NULL,
    extension_id text NOT NULL,
    provider_id text DEFAULT ''::text NOT NULL,
    priority bigint NOT NULL,
    value_json text NOT NULL,
    source_fingerprint text NOT NULL,
    updated_at bigint NOT NULL,
    CONSTRAINT automatic_metadata_provenance_field_name_check CHECK (((length(field_name) >= 1) AND (length(field_name) <= 64))),
    CONSTRAINT automatic_metadata_provenance_priority_check CHECK (((priority >= 0) AND (priority <= 1000)))
);



CREATE TABLE automatic_timeline_markers (
    asset_id text NOT NULL,
    extension_id text NOT NULL,
    marker_type text NOT NULL,
    start_seconds double precision NOT NULL,
    end_seconds double precision NOT NULL,
    confidence double precision NOT NULL,
    source_fingerprint text NOT NULL,
    generation text NOT NULL,
    task_id text NOT NULL,
    updated_at bigint NOT NULL,
    CONSTRAINT automatic_timeline_markers_check CHECK ((end_seconds > start_seconds)),
    CONSTRAINT automatic_timeline_markers_confidence_check CHECK (((confidence >= (0)::double precision) AND (confidence <= (1)::double precision))),
    CONSTRAINT automatic_timeline_markers_marker_type_check CHECK ((marker_type = ANY (ARRAY['intro'::text, 'credits'::text, 'recap'::text, 'preview'::text]))),
    CONSTRAINT automatic_timeline_markers_start_seconds_check CHECK ((start_seconds >= (0)::double precision))
);



CREATE TABLE automation_dependencies (
    task_id text NOT NULL,
    prerequisite_id text NOT NULL,
    CONSTRAINT automation_dependencies_check CHECK ((task_id <> prerequisite_id))
);



CREATE TABLE automation_events (
    id text NOT NULL,
    event_key text NOT NULL,
    kind text NOT NULL,
    library_id text NOT NULL,
    asset_id text NOT NULL,
    fingerprint text NOT NULL,
    payload_json text NOT NULL,
    created_at bigint NOT NULL,
    dispatched_at bigint,
    cursor_extension text DEFAULT ''::text NOT NULL
);



CREATE TABLE automation_observations (
    asset_id text NOT NULL,
    library_id text NOT NULL,
    fingerprint text NOT NULL,
    revision bigint NOT NULL
);



CREATE TABLE automation_policies (
    extension_id text NOT NULL,
    library_id text NOT NULL,
    mode text DEFAULT 'automatic'::text NOT NULL,
    priority bigint DEFAULT 100 NOT NULL,
    operation text DEFAULT 'metadata.propose'::text NOT NULL,
    config_revision text DEFAULT ''::text NOT NULL,
    algorithm_version text DEFAULT '1'::text NOT NULL,
    interval_seconds bigint DEFAULT 86400 NOT NULL,
    next_run_at bigint DEFAULT 0 NOT NULL,
    max_concurrent bigint DEFAULT 1 NOT NULL,
    CONSTRAINT automation_policies_interval_seconds_check CHECK ((interval_seconds >= 60)),
    CONSTRAINT automation_policies_max_concurrent_check CHECK (((max_concurrent >= 1) AND (max_concurrent <= 16))),
    CONSTRAINT automation_policies_mode_check CHECK ((mode = ANY (ARRAY['automatic'::text, 'manual'::text, 'disabled'::text, 'scheduled'::text]))),
    CONSTRAINT automation_policies_priority_check CHECK (((priority >= 0) AND (priority <= 1000)))
);



CREATE TABLE automation_provider_cache (
    cache_key text NOT NULL,
    provider text NOT NULL,
    operation text NOT NULL,
    query_hash text NOT NULL,
    response_json text NOT NULL,
    expires_at bigint NOT NULL,
    created_at bigint NOT NULL
);



CREATE TABLE automation_provider_limits (
    provider text NOT NULL,
    next_allowed_at bigint DEFAULT 0 NOT NULL,
    min_interval_seconds bigint DEFAULT 1 NOT NULL,
    max_concurrent bigint DEFAULT 2 NOT NULL,
    CONSTRAINT automation_provider_limits_max_concurrent_check CHECK (((max_concurrent >= 1) AND (max_concurrent <= 16))),
    CONSTRAINT automation_provider_limits_min_interval_seconds_check CHECK ((min_interval_seconds >= 0))
);



CREATE TABLE automation_results (
    extension_id text NOT NULL,
    asset_id text NOT NULL,
    fingerprint text NOT NULL,
    task_id text NOT NULL,
    result_json text NOT NULL,
    updated_at bigint NOT NULL
);



CREATE TABLE automation_sweeps (
    id text NOT NULL,
    library_id text NOT NULL,
    extension_id text DEFAULT ''::text NOT NULL,
    kind text NOT NULL,
    cursor_asset text DEFAULT ''::text NOT NULL,
    created_at bigint NOT NULL,
    completed_at bigint
);



CREATE TABLE automation_task_links (
    task_id text NOT NULL,
    event_id text NOT NULL,
    extension_id text NOT NULL,
    operation text NOT NULL,
    fingerprint text NOT NULL,
    generation text NOT NULL,
    provider text NOT NULL
);



CREATE TABLE backup_schedule (
    singleton bigint NOT NULL,
    enabled bigint DEFAULT 0 NOT NULL,
    interval_minutes bigint DEFAULT 1440 NOT NULL,
    next_due_at bigint DEFAULT 0 NOT NULL,
    last_started_at bigint,
    last_completed_at bigint,
    last_outcome text DEFAULT 'never'::text NOT NULL,
    CONSTRAINT backup_schedule_enabled_check CHECK ((enabled = ANY (ARRAY[(0)::bigint, (1)::bigint]))),
    CONSTRAINT backup_schedule_interval_minutes_check CHECK (((interval_minutes >= 60) AND (interval_minutes <= 10080))),
    CONSTRAINT backup_schedule_last_outcome_check CHECK ((last_outcome = ANY (ARRAY['never'::text, 'running'::text, 'succeeded'::text, 'failed'::text, 'interrupted'::text]))),
    CONSTRAINT backup_schedule_singleton_check CHECK ((singleton = 1))
);



CREATE TABLE bootstrap_state (
    singleton integer NOT NULL,
    completed_at bigint DEFAULT (EXTRACT(epoch FROM now()))::bigint NOT NULL,
    CONSTRAINT bootstrap_state_singleton_check CHECK ((singleton = 1))
);



CREATE TABLE durable_tasks (
    id text NOT NULL,
    library_id text,
    owner text NOT NULL,
    idempotency_key text NOT NULL,
    node_id text NOT NULL,
    category text DEFAULT 'scan'::text NOT NULL,
    target_id text DEFAULT ''::text NOT NULL,
    component text DEFAULT ''::text NOT NULL,
    width bigint DEFAULT 640 NOT NULL,
    result_json text DEFAULT '{}'::text NOT NULL,
    refresh_mode text DEFAULT 'incremental'::text NOT NULL,
    status text DEFAULT 'queued'::text NOT NULL,
    priority bigint DEFAULT 100 NOT NULL,
    attempt bigint DEFAULT 0 NOT NULL,
    failure_count bigint DEFAULT 0 NOT NULL,
    max_attempts bigint DEFAULT 3 NOT NULL,
    progress bigint DEFAULT 0 NOT NULL,
    processed_items bigint DEFAULT 0 NOT NULL,
    created_at bigint NOT NULL,
    updated_at bigint NOT NULL,
    started_at bigint,
    finished_at bigint,
    lease_until bigint,
    available_at bigint NOT NULL,
    error text DEFAULT ''::text NOT NULL,
    CONSTRAINT durable_tasks_attempt_check CHECK ((attempt >= 0)),
    CONSTRAINT durable_tasks_category_check CHECK ((category = ANY (ARRAY['scan'::text, 'analyze'::text, 'prepare'::text, 'image'::text, 'trickplay'::text, 'system'::text]))),
    CONSTRAINT durable_tasks_failure_count_check CHECK ((failure_count >= 0)),
    CONSTRAINT durable_tasks_max_attempts_check CHECK (((max_attempts >= 1) AND (max_attempts <= 1000000))),
    CONSTRAINT durable_tasks_priority_check CHECK (((priority >= 0) AND (priority <= 1000))),
    CONSTRAINT durable_tasks_processed_items_check CHECK ((processed_items >= 0)),
    CONSTRAINT durable_tasks_progress_check CHECK (((progress >= 0) AND (progress <= 100))),
    CONSTRAINT durable_tasks_refresh_mode_check CHECK ((refresh_mode = ANY (ARRAY['incremental'::text, 'new_only'::text, 'missing_metadata'::text, 'full'::text]))),
    CONSTRAINT durable_tasks_scope_check CHECK ((((category = 'system'::text) AND (library_id IS NULL) AND (node_id = 'local'::text)) OR ((category <> 'system'::text) AND (library_id IS NOT NULL)))),
    CONSTRAINT durable_tasks_status_check CHECK ((status = ANY (ARRAY['queued'::text, 'running'::text, 'paused'::text, 'completed'::text, 'cancelled'::text, 'error'::text]))),
    CONSTRAINT durable_tasks_width_check CHECK (((category = 'trickplay' AND width BETWEEN 160 AND 1920) OR width IN (320,640,1280)))
);



CREATE TABLE library_folders (
    library_id text NOT NULL,
    parent_id text NOT NULL,
    active integer DEFAULT 1 NOT NULL,
    CONSTRAINT library_folders_active_check CHECK ((active = ANY (ARRAY[0, 1]))),
    CONSTRAINT library_folders_check CHECK ((library_id <> parent_id))
);



CREATE TABLE library_permissions (
    library_id text NOT NULL,
    user_id text NOT NULL
);



CREATE TABLE user_group_libraries (
    group_id text NOT NULL,
    library_id text NOT NULL
);



CREATE TABLE user_group_members (
    group_id text NOT NULL,
    user_id text NOT NULL
);



CREATE TABLE user_groups (
    id text NOT NULL,
    name text NOT NULL,
    is_default integer DEFAULT 0 NOT NULL,
    CONSTRAINT user_groups_name_check CHECK (((length(name) >= 1) AND (length(name) <= 80)))
);



CREATE TABLE users (
    id text NOT NULL,
    role text NOT NULL,
    created_at bigint DEFAULT (EXTRACT(epoch FROM now()))::bigint NOT NULL,
    disabled_at bigint,
    display_name text,
    sso_username text,
    CONSTRAINT users_role_check CHECK ((role = ANY (ARRAY['administrator'::text, 'user'::text])))
);






CREATE TABLE extension_audit (
    id text NOT NULL,
    actor_id text NOT NULL,
    extension_id text NOT NULL,
    action text NOT NULL,
    created_at bigint NOT NULL
);
CREATE INDEX extension_audit_retention ON extension_audit(created_at,id);



CREATE TABLE extension_avatar_images (
    id text NOT NULL,
    pack_id text NOT NULL,
    name text NOT NULL,
    sha256 text NOT NULL
);



CREATE TABLE avatar_packs (
 id text PRIMARY KEY, name text NOT NULL, source text NOT NULL
);
CREATE TABLE avatar_images (
 id text PRIMARY KEY, pack_id text NOT NULL REFERENCES avatar_packs(id) ON DELETE CASCADE,
 name text NOT NULL, sha256 text NOT NULL
);
CREATE INDEX avatar_images_asset ON avatar_images(sha256);
CREATE INDEX avatar_images_pack ON avatar_images(pack_id);
CREATE TABLE avatar_policy (
 singleton integer PRIMARY KEY CHECK(singleton=1),
 allow_custom bigint NOT NULL CHECK(allow_custom IN (0,1)),
 allow_gravatar bigint NOT NULL CHECK(allow_gravatar IN (0,1)),
 assign_on_create bigint NOT NULL CHECK(assign_on_create IN (0,1))
);

CREATE TABLE extension_avatar_packs (
    id text NOT NULL,
    extension_id text NOT NULL,
    name text NOT NULL,
    source text NOT NULL
);



CREATE TABLE extension_data (
    extension_id text NOT NULL,
    scope text NOT NULL,
    scope_id text NOT NULL,
    key text NOT NULL,
    value text NOT NULL,
    version text NOT NULL,
    expires_at bigint,
    durability text NOT NULL,
    value_bytes bigint NOT NULL,
    sha256 text NOT NULL,
    CONSTRAINT extension_data_durability_check CHECK ((durability = ANY (ARRAY['cache'::text, 'durable'::text]))),
    CONSTRAINT extension_data_value_bytes_check CHECK ((value_bytes >= 0))
);



CREATE TABLE extension_data_namespaces (
    extension_id text NOT NULL,
    publisher_id text NOT NULL,
    publisher_key text NOT NULL,
    entries bigint DEFAULT 0 NOT NULL,
    bytes bigint DEFAULT 0 NOT NULL,
    CONSTRAINT extension_data_namespaces_bytes_check CHECK ((bytes >= 0)),
    CONSTRAINT extension_data_namespaces_entries_check CHECK ((entries >= 0))
);



CREATE TABLE extension_grants (
    extension_id text NOT NULL,
    library_id text NOT NULL
);



CREATE TABLE extension_object_chunks (
    object_id text NOT NULL,
    offset_bytes bigint NOT NULL,
    payload bytea NOT NULL,
    sha256 text NOT NULL,
    CONSTRAINT extension_object_chunks_offset_bytes_check CHECK ((offset_bytes >= 0)),
    CONSTRAINT extension_object_chunks_payload_check CHECK ((octet_length(payload) <= 16384))
);



CREATE TABLE extension_objects (
    id text NOT NULL,
    extension_id text NOT NULL,
    scope_kind text NOT NULL,
    scope_id text NOT NULL,
    size bigint NOT NULL,
    received bigint DEFAULT 0 NOT NULL,
    sha256 text NOT NULL,
    durability text NOT NULL,
    expires_at bigint,
    upload_expires_at bigint NOT NULL,
    complete bigint DEFAULT 0 NOT NULL,
    CONSTRAINT extension_objects_check CHECK (((received >= 0) AND (received <= size))),
    CONSTRAINT extension_objects_complete_check CHECK ((complete = ANY (ARRAY[(0)::bigint, (1)::bigint]))),
    CONSTRAINT extension_objects_durability_check CHECK ((durability = ANY (ARRAY['cache'::text, 'durable'::text]))),
    CONSTRAINT extension_objects_size_check CHECK (((size >= 0) AND (size <= 16777216)))
);



CREATE TABLE extension_publishers (
    id text NOT NULL,
    public_key text NOT NULL,
    active bigint DEFAULT 1 NOT NULL,
    CONSTRAINT extension_publishers_active_check CHECK ((active = ANY (ARRAY[(0)::bigint, (1)::bigint])))
);



CREATE TABLE extension_repositories (
    id text NOT NULL,
    name text NOT NULL,
    url text NOT NULL,
    manifest_json text NOT NULL,
    refreshed_at bigint NOT NULL
);



CREATE TABLE extension_runs (
    id text NOT NULL,
    extension_id text NOT NULL,
    library_id text NOT NULL,
    work_id text NOT NULL,
    actor_id text NOT NULL,
    started_at bigint NOT NULL,
    duration_ms bigint DEFAULT 0 NOT NULL,
    status text NOT NULL,
    result_json text DEFAULT '{}'::text NOT NULL
);



CREATE TABLE extension_settings (
    extension_id text NOT NULL,
    settings_json text DEFAULT '{}'::text NOT NULL
);



CREATE TABLE extensions (
    id text NOT NULL,
    publisher_id text NOT NULL,
    package_json text NOT NULL,
    enabled bigint DEFAULT 0 NOT NULL,
    generation text NOT NULL,
    all_libraries integer DEFAULT 0 NOT NULL,
    CONSTRAINT extensions_all_libraries_check CHECK ((all_libraries = ANY (ARRAY[0, 1]))),
    CONSTRAINT extensions_enabled_check CHECK ((enabled = ANY (ARRAY[(0)::bigint, (1)::bigint])))
);



CREATE TABLE inventory_assets (
    id text NOT NULL,
    source_id text NOT NULL,
    library_id text NOT NULL,
    relative_path text NOT NULL,
    size bigint NOT NULL,
    modified_ns text NOT NULL,
    mime text NOT NULL,
    probe_json text NOT NULL,
    available integer NOT NULL,
    CONSTRAINT inventory_assets_available_check CHECK ((available = ANY (ARRAY[0, 1])))
);



CREATE TABLE inventory_audio (
    asset_id text NOT NULL,
    audios_json text NOT NULL
);



CREATE TABLE inventory_cache_generation (
    id integer NOT NULL,
    gen integer DEFAULT 1 NOT NULL,
    CONSTRAINT inventory_cache_generation_id_check CHECK ((id = 1))
);



CREATE TABLE inventory_edition_metadata (
    edition_id text NOT NULL,
    edition_key text DEFAULT 'standard'::text NOT NULL,
    label text DEFAULT 'Édition locale'::text NOT NULL
);



CREATE TABLE inventory_editions (
    id text NOT NULL,
    work_id text NOT NULL
);



CREATE TABLE inventory_editorial (
    asset_id text NOT NULL,
    fallback_json text NOT NULL,
    nfo_json text,
    nfo_path text,
    nfo_priority integer
);



CREATE TABLE inventory_episodes (
    id text NOT NULL,
    season_id text NOT NULL,
    episode_number integer NOT NULL,
    work_id text NOT NULL
);



CREATE TABLE inventory_images (
    id text NOT NULL,
    work_id text NOT NULL,
    library_id text NOT NULL,
    root_snapshot text NOT NULL,
    relative_path text NOT NULL,
    size bigint NOT NULL,
    modified_ns text NOT NULL,
    mime text NOT NULL,
    priority integer NOT NULL,
    available integer DEFAULT 1 NOT NULL,
    role text DEFAULT 'poster'::text NOT NULL,
    CONSTRAINT inventory_images_available_check CHECK ((available = ANY (ARRAY[0, 1])))
);



CREATE TABLE inventory_seasons (
    id text NOT NULL,
    series_id text NOT NULL,
    season_number integer NOT NULL,
    title text DEFAULT ''::text NOT NULL
);



CREATE TABLE inventory_sources (
    root_snapshot text NOT NULL,
    id text NOT NULL,
    version_id text NOT NULL,
    library_id text NOT NULL
);



CREATE TABLE inventory_subtitles (
    asset_id text NOT NULL,
    subtitles_json text NOT NULL
);



CREATE TABLE inventory_tracks (
    id text NOT NULL,
    asset_id text NOT NULL,
    stream_index integer NOT NULL,
    metadata_json text NOT NULL
);



CREATE TABLE inventory_version_metadata (
    version_id text NOT NULL,
    version_key text DEFAULT 'default'::text NOT NULL,
    label text DEFAULT 'Version locale'::text NOT NULL
);



CREATE TABLE inventory_versions (
    id text NOT NULL,
    edition_id text NOT NULL
);



CREATE TABLE inventory_works (
    id text NOT NULL,
    library_id text NOT NULL,
    identity_key text NOT NULL,
    title text NOT NULL,
    year integer,
    external_ids_json text NOT NULL,
    editorial_json text DEFAULT '{}'::text NOT NULL,
    title_sort text DEFAULT ''::text NOT NULL,
    kind text DEFAULT 'film'::text NOT NULL,
    added_at bigint,
    manual_json text,
    minimum_age integer,
    display_title_sort text DEFAULT ''::text NOT NULL,
    effective_year integer,
    max_height integer DEFAULT 0 NOT NULL,
    codecs_csv text DEFAULT ','::text NOT NULL,
    languages_csv text DEFAULT ','::text NOT NULL,
    genres_csv text DEFAULT '|'::text NOT NULL,
    has_available_asset integer DEFAULT 0 NOT NULL,
    season_count_cached integer DEFAULT 0 NOT NULL,
    episode_count_cached integer DEFAULT 0 NOT NULL,
    CONSTRAINT inventory_works_has_available_asset_check CHECK ((has_available_asset = ANY (ARRAY[0, 1]))),
    CONSTRAINT inventory_works_minimum_age_check CHECK (((minimum_age >= 0) AND (minimum_age <= 21)))
);



CREATE TABLE inventory_works_unicode_migration (
    completed_at text NOT NULL
);



CREATE TABLE libraries (
    id text NOT NULL,
    name text NOT NULL,
    kind text NOT NULL,
    node_id text NOT NULL,
    root text NOT NULL,
    enabled integer NOT NULL,
    excluded_patterns text DEFAULT ''::text NOT NULL,
    options_json text DEFAULT '{}'::text NOT NULL,
    next_metadata_refresh_at bigint DEFAULT 0 NOT NULL,
    CONSTRAINT libraries_enabled_check CHECK ((enabled = ANY (ARRAY[0, 1]))),
    CONSTRAINT libraries_kind_check CHECK ((kind = ANY (ARRAY['films'::text, 'series'::text, 'music'::text]))),
    CONSTRAINT libraries_name_check CHECK (((length(name) >= 1) AND (length(name) <= 100)))
);



CREATE TABLE library_artwork (
    library_id text NOT NULL,
    kind text NOT NULL,
    sha256 text NOT NULL,
    revision text NOT NULL,
    CONSTRAINT library_artwork_kind_check CHECK ((kind = ANY (ARRAY['poster'::text, 'backdrop'::text])))
);



CREATE TABLE library_initialization (
    singleton integer NOT NULL,
    CONSTRAINT library_initialization_singleton_check CHECK ((singleton = 1))
);



CREATE TABLE local_credentials (
    user_id text NOT NULL,
    login text NOT NULL,
    password_hash text NOT NULL,
    updated_at bigint DEFAULT (EXTRACT(epoch FROM now()))::bigint NOT NULL,
    CONSTRAINT local_credentials_login_check CHECK ((((length(login) >= 3) AND (length(login) <= 64)) AND (login ~ '^[a-z0-9._-]+$'::text)))
);



CREATE TABLE login_throttle (
    throttle_key text NOT NULL,
    window_started bigint NOT NULL,
    attempts integer NOT NULL
);



CREATE TABLE manual_timeline_markers (
    asset_id text NOT NULL,
    marker_type text NOT NULL,
    start_seconds double precision,
    end_seconds double precision,
    enabled smallint NOT NULL,
    updated_by text NOT NULL,
    updated_at bigint NOT NULL,
    CONSTRAINT manual_timeline_markers_check CHECK ((((enabled = 0) AND (start_seconds IS NULL) AND (end_seconds IS NULL)) OR ((enabled = 1) AND (start_seconds >= (0)::double precision) AND (end_seconds > start_seconds)))),
    CONSTRAINT manual_timeline_markers_enabled_check CHECK ((enabled = ANY (ARRAY[0, 1]))),
    CONSTRAINT manual_timeline_markers_marker_type_check CHECK ((marker_type = ANY (ARRAY['intro'::text, 'credits'::text, 'recap'::text, 'preview'::text])))
);



CREATE TABLE media_group_members (
    work_id text NOT NULL,
    group_id text NOT NULL
);



CREATE TABLE media_groups (
    id text NOT NULL,
    label text NOT NULL
);



CREATE TABLE media_match_candidates (
    id text NOT NULL,
    left_work_id text NOT NULL,
    right_work_id text NOT NULL,
    kind text NOT NULL,
    score integer NOT NULL,
    reason text NOT NULL,
    status text DEFAULT 'pending'::text NOT NULL,
    merge_id text,
    created_at bigint DEFAULT (EXTRACT(epoch FROM now()))::bigint NOT NULL,
    updated_at bigint DEFAULT (EXTRACT(epoch FROM now()))::bigint NOT NULL,
    CONSTRAINT media_match_candidates_status_check CHECK ((status = ANY (ARRAY['pending'::text, 'confirmed'::text, 'rejected'::text, 'undone'::text])))
);



CREATE TABLE media_merge_history (
    id text NOT NULL,
    group_id text NOT NULL,
    kind text NOT NULL,
    origin text NOT NULL,
    candidate_id text,
    snapshot_json text NOT NULL,
    created_at bigint DEFAULT (EXTRACT(epoch FROM now()))::bigint NOT NULL,
    undone_at bigint,
    CONSTRAINT media_merge_history_origin_check CHECK ((origin = ANY (ARRAY['auto'::text, 'manual'::text, 'candidate'::text])))
);



CREATE TABLE media_nodes (
    id text NOT NULL,
    name text DEFAULT 'Local Media Node'::text NOT NULL,
    kind text DEFAULT 'local'::text NOT NULL,
    base_url text,
    public_url text,
    status text DEFAULT 'connected'::text NOT NULL,
    key_id text DEFAULT 'local'::text NOT NULL,
    shared_secret text DEFAULT ''::text NOT NULL,
    roots_json text DEFAULT '[]'::text NOT NULL,
    paired_at bigint DEFAULT (EXTRACT(epoch FROM now()))::bigint NOT NULL,
    last_seen_at bigint,
    revoked_at bigint,
    version text,
    connection_mode text DEFAULT 'outbound'::text NOT NULL,
    CONSTRAINT media_nodes_id_check CHECK (((length(id) >= 1) AND (length(id) <= 128))),
    CONSTRAINT media_nodes_kind_check CHECK ((kind = ANY (ARRAY['local'::text, 'remote'::text]))),
    CONSTRAINT media_nodes_name_check CHECK (((length(name) >= 1) AND (length(name) <= 100))),
    CONSTRAINT media_nodes_status_check CHECK ((status = ANY (ARRAY['connected'::text, 'disconnected'::text, 'revoked'::text])))
);



CREATE TABLE media_work_external_ids (
    work_id text NOT NULL,
    kind text NOT NULL,
    provider text NOT NULL,
    external_id text NOT NULL
);



CREATE TABLE music_scan_runs (
    library_id text NOT NULL,
    scan_id text NOT NULL
);



CREATE TABLE music_tracks (
    id text NOT NULL,
    library_id text NOT NULL,
    root_snapshot text NOT NULL,
    relative_path text NOT NULL,
    size bigint NOT NULL,
    modified_ns text NOT NULL,
    title text NOT NULL,
    artist text NOT NULL,
    album text NOT NULL,
    album_artist text NOT NULL,
    track_number integer,
    disc_number integer,
    duration double precision,
    codec text NOT NULL,
    mime text NOT NULL,
    scan_id text NOT NULL,
    available integer DEFAULT 1 NOT NULL,
    lyrics text DEFAULT ''::text NOT NULL,
    cover_path text,
    cover_size bigint,
    cover_modified_ns text,
    cover_mime text
);



CREATE TABLE node_enrollment_tokens (
    id text NOT NULL,
    token_hash text NOT NULL,
    created_at bigint DEFAULT (EXTRACT(epoch FROM now()))::bigint NOT NULL,
    expires_at bigint NOT NULL,
    used_at bigint,
    used_by_node_id text,
    CONSTRAINT node_enrollment_tokens_id_check CHECK (((length(id) >= 1) AND (length(id) <= 128)))
);



CREATE TABLE oidc_flows (
    state_hash text NOT NULL,
    browser_hash text NOT NULL,
    nonce text NOT NULL,
    verifier text NOT NULL,
    link_session_hash text,
    expires_at bigint NOT NULL
);



CREATE TABLE oidc_identities (
    issuer text NOT NULL,
    subject text NOT NULL,
    user_id text NOT NULL,
    CONSTRAINT oidc_identities_subject_check CHECK (((length(subject) >= 1) AND (length(subject) <= 255)))
);



CREATE TABLE parental_blocked_tags (
    user_id text NOT NULL,
    tag_json text NOT NULL
);



CREATE TABLE parental_policies (
    user_id text NOT NULL,
    maximum_age integer NOT NULL,
    allow_unrated integer NOT NULL,
    blocked_tags_json text DEFAULT '[]'::text NOT NULL,
    access_schedule_json text DEFAULT '[]'::text NOT NULL,
    CONSTRAINT parental_policies_allow_unrated_check CHECK ((allow_unrated = ANY (ARRAY[0, 1]))),
    CONSTRAINT parental_policies_maximum_age_check CHECK (((maximum_age >= 0) AND (maximum_age <= 21)))
);



CREATE TABLE personal_list_items (
    list_id text NOT NULL,
    work_id text NOT NULL,
    added_at bigint DEFAULT (EXTRACT(epoch FROM now()))::bigint NOT NULL,
    "position" integer DEFAULT 0 NOT NULL
);



CREATE TABLE personal_lists (
    id text NOT NULL,
    user_id text NOT NULL,
    name text NOT NULL,
    visibility text DEFAULT 'private'::text NOT NULL,
    CONSTRAINT personal_lists_name_check CHECK (((length(name) >= 1) AND (length(name) <= 100))),
    CONSTRAINT personal_lists_visibility_check CHECK ((visibility = ANY (ARRAY['private'::text, 'shared'::text, 'public'::text])))
);



CREATE TABLE playback_progress (
    user_id text NOT NULL,
    work_id text NOT NULL,
    position_seconds double precision NOT NULL,
    duration_seconds double precision NOT NULL,
    watched integer NOT NULL,
    revision bigint NOT NULL,
    updated_at bigint NOT NULL,
    CONSTRAINT playback_progress_duration_seconds_check CHECK ((duration_seconds > (0)::double precision)),
    CONSTRAINT playback_progress_position_seconds_check CHECK ((position_seconds >= (0)::double precision)),
    CONSTRAINT playback_progress_revision_check CHECK ((revision > 0)),
    CONSTRAINT playback_progress_watched_check CHECK ((watched = ANY (ARRAY[0, 1])))
);



CREATE TABLE playlist_music_items (
    list_id text NOT NULL,
    track_id text NOT NULL,
    "position" integer NOT NULL
);



CREATE TABLE playlist_policy (
    singleton integer NOT NULL,
    allow_public integer DEFAULT 0 NOT NULL,
    allow_sharing integer DEFAULT 0 NOT NULL,
    CONSTRAINT playlist_policy_allow_public_check CHECK ((allow_public = ANY (ARRAY[0, 1]))),
    CONSTRAINT playlist_policy_allow_sharing_check CHECK ((allow_sharing = ANY (ARRAY[0, 1]))),
    CONSTRAINT playlist_policy_singleton_check CHECK ((singleton = 1))
);



CREATE TABLE playlist_shares (
    list_id text NOT NULL,
    user_id text NOT NULL
);



CREATE TABLE poster_previews (
    work_id text NOT NULL,
    width integer NOT NULL,
    revision text NOT NULL,
    mime text NOT NULL,
    sha256 text NOT NULL
);



CREATE TABLE scan_batch_receipts (
    scan_id text NOT NULL,
    sequence bigint NOT NULL,
    CONSTRAINT scan_batch_receipts_sequence_check CHECK ((sequence >= 0))
);



CREATE TABLE scan_jobs (
    library_id text NOT NULL,
    status text NOT NULL,
    discovered integer DEFAULT 0 NOT NULL,
    probed integer DEFAULT 0 NOT NULL,
    reused integer DEFAULT 0 NOT NULL,
    errors_json text DEFAULT '[]'::text NOT NULL,
    errors_count integer DEFAULT 0 NOT NULL,
    warnings_count integer DEFAULT 0 NOT NULL,
    refresh_mode text DEFAULT 'incremental'::text NOT NULL,
    CONSTRAINT scan_jobs_refresh_mode_check CHECK ((refresh_mode = ANY (ARRAY['incremental'::text, 'new_only'::text, 'missing_metadata'::text, 'full'::text])))
);



CREATE TABLE scan_staging_files (
    scan_id text NOT NULL,
    library_id text NOT NULL,
    relative_path text NOT NULL,
    size bigint NOT NULL,
    modified_ns text NOT NULL,
    mime text NOT NULL,
    title text NOT NULL,
    year integer,
    identity_key text NOT NULL,
    external_ids_json text NOT NULL,
    probe_json text,
    reused integer NOT NULL,
    poster_path text,
    poster_size bigint,
    poster_modified_ns text,
    poster_mime text,
    poster_priority integer,
    editorial_outcome text NOT NULL,
    editorial_json text,
    editorial_path text,
    editorial_priority integer,
    series_identity_key text,
    series_title text,
    series_year integer,
    series_external_ids_json text,
    series_editorial_json text,
    series_poster_path text,
    series_poster_size bigint,
    series_poster_modified_ns text,
    series_poster_mime text,
    series_poster_priority integer,
    season_number integer,
    episode_number integer,
    subtitles_json text DEFAULT '[]'::text NOT NULL,
    audios_json text DEFAULT '[]'::text NOT NULL,
    artwork_json text DEFAULT '{}'::text NOT NULL
);



CREATE TABLE scan_staging_issues (
    scan_id text NOT NULL,
    kind text NOT NULL,
    relative_path text NOT NULL,
    message text NOT NULL
);



CREATE TABLE scheduled_scans (
    library_id text NOT NULL,
    interval_minutes bigint NOT NULL,
    enabled bigint NOT NULL,
    next_run_at bigint NOT NULL,
    last_run_at bigint,
    last_outcome text DEFAULT 'never'::text NOT NULL,
    last_request_key text DEFAULT ''::text NOT NULL,
    CONSTRAINT scheduled_scans_enabled_check CHECK ((enabled = ANY (ARRAY[(0)::bigint, (1)::bigint]))),
    CONSTRAINT scheduled_scans_interval_minutes_check CHECK (((interval_minutes >= 5) AND (interval_minutes <= 10080)))
);



CREATE TABLE server_settings (
    singleton integer NOT NULL,
    settings_json text NOT NULL,
    CONSTRAINT server_settings_singleton_check CHECK ((singleton = 1))
);



CREATE TABLE sessions (
    token_hash text NOT NULL,
    user_id text NOT NULL,
    created_at bigint NOT NULL,
    last_seen_at bigint NOT NULL,
    expires_at bigint NOT NULL,
    device_label text DEFAULT 'Navigateur'::text NOT NULL
);



CREATE TABLE system_task_schedules (
    id text NOT NULL,
    name text NOT NULL,
    description text NOT NULL,
    category text NOT NULL,
    interval_minutes bigint NOT NULL,
    enabled bigint NOT NULL,
    next_due_at bigint NOT NULL,
    last_started_at bigint,
    last_completed_at bigint,
    last_outcome text DEFAULT 'never'::text NOT NULL,
    last_error_kind text,
    last_error_scope text,
    last_message text,
    CONSTRAINT system_task_schedules_enabled_check CHECK ((enabled = ANY (ARRAY[(0)::bigint, (1)::bigint]))),
    CONSTRAINT system_task_schedules_interval_minutes_check CHECK (((interval_minutes >= 5) AND (interval_minutes <= 10080))),
    CONSTRAINT system_task_schedules_last_outcome_check CHECK ((last_outcome = ANY (ARRAY['never'::text, 'queued'::text, 'running'::text, 'succeeded'::text, 'failed'::text, 'interrupted'::text])))
);



CREATE TABLE user_avatars (
    user_id text NOT NULL,
    sha256 text NOT NULL
);



CREATE TABLE user_media_policies (
    user_id text NOT NULL,
    allow_media_download integer DEFAULT 1 NOT NULL,
    CONSTRAINT user_media_policies_allow_media_download_check CHECK ((allow_media_download = ANY (ARRAY[0, 1])))
);



CREATE TABLE user_preferences (
    user_id text NOT NULL,
    preferences_json text DEFAULT '{}'::text NOT NULL
);



CREATE TABLE work_feedback (
    user_id text NOT NULL,
    work_id text NOT NULL,
    favorite integer DEFAULT 0 NOT NULL,
    rating double precision,
    CONSTRAINT work_feedback_favorite_check CHECK ((favorite = ANY (ARRAY[0, 1]))),
    CONSTRAINT work_feedback_rating_check CHECK ((((rating >= (0.5)::double precision) AND (rating <= (5)::double precision)) AND ((rating * (2)::double precision) = (((rating * (2)::double precision))::integer)::double precision)))
);



CREATE TABLE work_image_data (
    image_id text NOT NULL,
    width integer NOT NULL,
    mime text NOT NULL,
    sha256 text NOT NULL
);



CREATE TABLE work_image_entries (
    id text NOT NULL,
    work_id text NOT NULL,
    kind text NOT NULL,
    "position" integer NOT NULL,
    width integer NOT NULL,
    height integer NOT NULL,
    language text,
    provider text,
    fingerprint text,
    source_url_hash text
);



CREATE TABLE work_image_hidden (
    work_id text NOT NULL,
    image_id text NOT NULL
);



CREATE TABLE work_image_order (
    work_id text NOT NULL,
    image_id text NOT NULL,
    "position" integer NOT NULL
);



CREATE TABLE work_posters (
    work_id text NOT NULL,
    sha256 text NOT NULL,
    revision text DEFAULT 'legacy'::text NOT NULL
);



ALTER TABLE ONLY active_playbacks
    ADD CONSTRAINT active_playbacks_pkey PRIMARY KEY (id);



ALTER TABLE ONLY active_scan_runs
    ADD CONSTRAINT active_scan_runs_pkey PRIMARY KEY (library_id);



ALTER TABLE ONLY api_key_libraries
    ADD CONSTRAINT api_key_libraries_pkey PRIMARY KEY (key_id, library_id);



ALTER TABLE ONLY api_keys
    ADD CONSTRAINT api_keys_pkey PRIMARY KEY (id);



ALTER TABLE ONLY api_keys
    ADD CONSTRAINT api_keys_token_hash_key UNIQUE (token_hash);



ALTER TABLE ONLY artwork_image_sources
    ADD CONSTRAINT artwork_image_sources_pkey PRIMARY KEY (work_id, source_url_hash);



ALTER TABLE ONLY assets
    ADD CONSTRAINT assets_pkey PRIMARY KEY (sha256);

ALTER TABLE avatar_images ADD CONSTRAINT avatar_images_sha256_fkey FOREIGN KEY (sha256) REFERENCES assets(sha256);



ALTER TABLE ONLY automatic_metadata_provenance
    ADD CONSTRAINT automatic_metadata_provenance_pkey PRIMARY KEY (work_id, field_name);



ALTER TABLE ONLY automatic_timeline_markers
    ADD CONSTRAINT automatic_timeline_markers_pkey PRIMARY KEY (asset_id, extension_id, marker_type);



ALTER TABLE ONLY automation_dependencies
    ADD CONSTRAINT automation_dependencies_pkey PRIMARY KEY (task_id, prerequisite_id);



ALTER TABLE ONLY automation_events
    ADD CONSTRAINT automation_events_event_key_key UNIQUE (event_key);



ALTER TABLE ONLY automation_events
    ADD CONSTRAINT automation_events_pkey PRIMARY KEY (id);



ALTER TABLE ONLY automation_observations
    ADD CONSTRAINT automation_observations_pkey PRIMARY KEY (asset_id);



ALTER TABLE ONLY automation_policies
    ADD CONSTRAINT automation_policies_pkey PRIMARY KEY (extension_id, library_id);



ALTER TABLE ONLY automation_provider_cache
    ADD CONSTRAINT automation_provider_cache_pkey PRIMARY KEY (cache_key);



ALTER TABLE ONLY automation_provider_limits
    ADD CONSTRAINT automation_provider_limits_pkey PRIMARY KEY (provider);



ALTER TABLE ONLY automation_results
    ADD CONSTRAINT automation_results_pkey PRIMARY KEY (extension_id, asset_id);



ALTER TABLE ONLY automation_sweeps
    ADD CONSTRAINT automation_sweeps_pkey PRIMARY KEY (id);



ALTER TABLE ONLY automation_task_links
    ADD CONSTRAINT automation_task_links_extension_id_fingerprint_key UNIQUE (extension_id, fingerprint);



ALTER TABLE ONLY automation_task_links
    ADD CONSTRAINT automation_task_links_pkey PRIMARY KEY (task_id);



ALTER TABLE ONLY backup_schedule
    ADD CONSTRAINT backup_schedule_pkey PRIMARY KEY (singleton);



ALTER TABLE ONLY bootstrap_state
    ADD CONSTRAINT bootstrap_state_pkey PRIMARY KEY (singleton);



ALTER TABLE ONLY durable_tasks
    ADD CONSTRAINT durable_tasks_owner_idempotency_key_key UNIQUE (owner, idempotency_key);



ALTER TABLE ONLY durable_tasks
    ADD CONSTRAINT durable_tasks_pkey PRIMARY KEY (id);



ALTER TABLE ONLY extension_audit
    ADD CONSTRAINT extension_audit_pkey PRIMARY KEY (id);



ALTER TABLE ONLY extension_avatar_images
    ADD CONSTRAINT extension_avatar_images_pkey PRIMARY KEY (id);



ALTER TABLE ONLY extension_avatar_packs
    ADD CONSTRAINT extension_avatar_packs_pkey PRIMARY KEY (id);



ALTER TABLE ONLY extension_data_namespaces
    ADD CONSTRAINT extension_data_namespaces_pkey PRIMARY KEY (extension_id);



ALTER TABLE ONLY extension_data
    ADD CONSTRAINT extension_data_pkey PRIMARY KEY (extension_id, scope, scope_id, key);



ALTER TABLE ONLY extension_grants
    ADD CONSTRAINT extension_grants_pkey PRIMARY KEY (extension_id, library_id);



ALTER TABLE ONLY extension_object_chunks
    ADD CONSTRAINT extension_object_chunks_pkey PRIMARY KEY (object_id, offset_bytes);



ALTER TABLE ONLY extension_objects
    ADD CONSTRAINT extension_objects_pkey PRIMARY KEY (id);



ALTER TABLE ONLY extension_publishers
    ADD CONSTRAINT extension_publishers_pkey PRIMARY KEY (id);



ALTER TABLE ONLY extension_repositories
    ADD CONSTRAINT extension_repositories_pkey PRIMARY KEY (id);



ALTER TABLE ONLY extension_repositories
    ADD CONSTRAINT extension_repositories_url_key UNIQUE (url);



ALTER TABLE ONLY extension_runs
    ADD CONSTRAINT extension_runs_pkey PRIMARY KEY (id);



ALTER TABLE ONLY extension_settings
    ADD CONSTRAINT extension_settings_pkey PRIMARY KEY (extension_id);



ALTER TABLE ONLY extensions
    ADD CONSTRAINT extensions_pkey PRIMARY KEY (id);



ALTER TABLE ONLY inventory_assets
    ADD CONSTRAINT inventory_assets_library_id_relative_path_key UNIQUE (library_id, relative_path);



ALTER TABLE ONLY inventory_assets
    ADD CONSTRAINT inventory_assets_pkey PRIMARY KEY (id);



ALTER TABLE ONLY inventory_audio
    ADD CONSTRAINT inventory_audio_pkey PRIMARY KEY (asset_id);



ALTER TABLE ONLY inventory_cache_generation
    ADD CONSTRAINT inventory_cache_generation_pkey PRIMARY KEY (id);



ALTER TABLE ONLY inventory_edition_metadata
    ADD CONSTRAINT inventory_edition_metadata_pkey PRIMARY KEY (edition_id);



ALTER TABLE ONLY inventory_editions
    ADD CONSTRAINT inventory_editions_pkey PRIMARY KEY (id);



ALTER TABLE ONLY inventory_editorial
    ADD CONSTRAINT inventory_editorial_pkey PRIMARY KEY (asset_id);



ALTER TABLE ONLY inventory_episodes
    ADD CONSTRAINT inventory_episodes_pkey PRIMARY KEY (id);



ALTER TABLE ONLY inventory_episodes
    ADD CONSTRAINT inventory_episodes_season_id_episode_number_key UNIQUE (season_id, episode_number);



ALTER TABLE ONLY inventory_episodes
    ADD CONSTRAINT inventory_episodes_work_id_key UNIQUE (work_id);



ALTER TABLE ONLY inventory_images
    ADD CONSTRAINT inventory_images_pkey PRIMARY KEY (id);



ALTER TABLE ONLY inventory_images
    ADD CONSTRAINT inventory_images_work_id_root_snapshot_relative_path_key UNIQUE (work_id, root_snapshot, relative_path);



ALTER TABLE ONLY inventory_seasons
    ADD CONSTRAINT inventory_seasons_pkey PRIMARY KEY (id);



ALTER TABLE ONLY inventory_seasons
    ADD CONSTRAINT inventory_seasons_series_id_season_number_key UNIQUE (series_id, season_number);



ALTER TABLE ONLY inventory_sources
    ADD CONSTRAINT inventory_sources_pkey PRIMARY KEY (id);



ALTER TABLE ONLY inventory_subtitles
    ADD CONSTRAINT inventory_subtitles_pkey PRIMARY KEY (asset_id);



ALTER TABLE ONLY inventory_tracks
    ADD CONSTRAINT inventory_tracks_asset_id_stream_index_key UNIQUE (asset_id, stream_index);



ALTER TABLE ONLY inventory_tracks
    ADD CONSTRAINT inventory_tracks_pkey PRIMARY KEY (id);



ALTER TABLE ONLY inventory_version_metadata
    ADD CONSTRAINT inventory_version_metadata_pkey PRIMARY KEY (version_id);



ALTER TABLE ONLY inventory_versions
    ADD CONSTRAINT inventory_versions_pkey PRIMARY KEY (id);



ALTER TABLE ONLY inventory_works
    ADD CONSTRAINT inventory_works_library_id_identity_key_key UNIQUE (library_id, identity_key);



ALTER TABLE ONLY inventory_works
    ADD CONSTRAINT inventory_works_pkey PRIMARY KEY (id);



ALTER TABLE ONLY libraries
    ADD CONSTRAINT libraries_pkey PRIMARY KEY (id);



ALTER TABLE ONLY library_artwork
    ADD CONSTRAINT library_artwork_pkey PRIMARY KEY (library_id, kind);



ALTER TABLE ONLY library_folders
    ADD CONSTRAINT library_folders_pkey PRIMARY KEY (library_id);



ALTER TABLE ONLY library_initialization
    ADD CONSTRAINT library_initialization_pkey PRIMARY KEY (singleton);



ALTER TABLE ONLY library_permissions
    ADD CONSTRAINT library_permissions_pkey PRIMARY KEY (library_id, user_id);



ALTER TABLE ONLY local_credentials
    ADD CONSTRAINT local_credentials_login_key UNIQUE (login);



ALTER TABLE ONLY local_credentials
    ADD CONSTRAINT local_credentials_pkey PRIMARY KEY (user_id);



ALTER TABLE ONLY login_throttle
    ADD CONSTRAINT login_throttle_pkey PRIMARY KEY (throttle_key);



ALTER TABLE ONLY manual_timeline_markers
    ADD CONSTRAINT manual_timeline_markers_pkey PRIMARY KEY (asset_id, marker_type);



ALTER TABLE ONLY media_group_members
    ADD CONSTRAINT media_group_members_pkey PRIMARY KEY (work_id);



ALTER TABLE ONLY media_groups
    ADD CONSTRAINT media_groups_pkey PRIMARY KEY (id);



ALTER TABLE ONLY media_match_candidates
    ADD CONSTRAINT media_match_candidates_left_work_id_right_work_id_key UNIQUE (left_work_id, right_work_id);



ALTER TABLE ONLY media_match_candidates
    ADD CONSTRAINT media_match_candidates_pkey PRIMARY KEY (id);



ALTER TABLE ONLY media_merge_history
    ADD CONSTRAINT media_merge_history_pkey PRIMARY KEY (id);



ALTER TABLE ONLY media_nodes
    ADD CONSTRAINT media_nodes_pkey PRIMARY KEY (id);



ALTER TABLE ONLY media_work_external_ids
    ADD CONSTRAINT media_work_external_ids_pkey PRIMARY KEY (work_id, provider);



ALTER TABLE ONLY music_scan_runs
    ADD CONSTRAINT music_scan_runs_pkey PRIMARY KEY (library_id);



ALTER TABLE ONLY music_tracks
    ADD CONSTRAINT music_tracks_library_id_root_snapshot_relative_path_key UNIQUE (library_id, root_snapshot, relative_path);



ALTER TABLE ONLY music_tracks
    ADD CONSTRAINT music_tracks_pkey PRIMARY KEY (id);



ALTER TABLE ONLY node_enrollment_tokens
    ADD CONSTRAINT node_enrollment_tokens_pkey PRIMARY KEY (id);



ALTER TABLE ONLY node_enrollment_tokens
    ADD CONSTRAINT node_enrollment_tokens_token_hash_key UNIQUE (token_hash);



ALTER TABLE ONLY oidc_flows
    ADD CONSTRAINT oidc_flows_pkey PRIMARY KEY (state_hash);



ALTER TABLE ONLY oidc_identities
    ADD CONSTRAINT oidc_identities_issuer_user_id_key UNIQUE (issuer, user_id);



ALTER TABLE ONLY oidc_identities
    ADD CONSTRAINT oidc_identities_pkey PRIMARY KEY (issuer, subject);



ALTER TABLE ONLY parental_blocked_tags
    ADD CONSTRAINT parental_blocked_tags_pkey PRIMARY KEY (user_id, tag_json);



ALTER TABLE ONLY parental_policies
    ADD CONSTRAINT parental_policies_pkey PRIMARY KEY (user_id);



ALTER TABLE ONLY personal_list_items
    ADD CONSTRAINT personal_list_items_pkey PRIMARY KEY (list_id, work_id);



ALTER TABLE ONLY personal_lists
    ADD CONSTRAINT personal_lists_pkey PRIMARY KEY (id);



ALTER TABLE ONLY playback_progress
    ADD CONSTRAINT playback_progress_pkey PRIMARY KEY (user_id, work_id);



ALTER TABLE ONLY playlist_music_items
    ADD CONSTRAINT playlist_music_items_pkey PRIMARY KEY (list_id, track_id);



ALTER TABLE ONLY playlist_policy
    ADD CONSTRAINT playlist_policy_pkey PRIMARY KEY (singleton);



ALTER TABLE ONLY playlist_shares
    ADD CONSTRAINT playlist_shares_pkey PRIMARY KEY (list_id, user_id);



ALTER TABLE ONLY poster_previews
    ADD CONSTRAINT poster_previews_pkey PRIMARY KEY (work_id, width);



ALTER TABLE ONLY scan_batch_receipts
    ADD CONSTRAINT scan_batch_receipts_pkey PRIMARY KEY (scan_id, sequence);



ALTER TABLE ONLY scan_jobs
    ADD CONSTRAINT scan_jobs_pkey PRIMARY KEY (library_id);



ALTER TABLE ONLY scan_staging_files
    ADD CONSTRAINT scan_staging_files_pkey PRIMARY KEY (scan_id, relative_path);



ALTER TABLE ONLY scheduled_scans
    ADD CONSTRAINT scheduled_scans_pkey PRIMARY KEY (library_id);



ALTER TABLE ONLY server_settings
    ADD CONSTRAINT server_settings_pkey PRIMARY KEY (singleton);



ALTER TABLE ONLY sessions
    ADD CONSTRAINT sessions_pkey PRIMARY KEY (token_hash);



ALTER TABLE ONLY system_task_schedules
    ADD CONSTRAINT system_task_schedules_pkey PRIMARY KEY (id);



ALTER TABLE ONLY user_avatars
    ADD CONSTRAINT user_avatars_pkey PRIMARY KEY (user_id);



ALTER TABLE ONLY user_group_libraries
    ADD CONSTRAINT user_group_libraries_pkey PRIMARY KEY (group_id, library_id);



ALTER TABLE ONLY user_group_members
    ADD CONSTRAINT user_group_members_pkey PRIMARY KEY (group_id, user_id);



ALTER TABLE ONLY user_groups
    ADD CONSTRAINT user_groups_name_key UNIQUE (name);



ALTER TABLE ONLY user_groups
    ADD CONSTRAINT user_groups_pkey PRIMARY KEY (id);



ALTER TABLE ONLY user_media_policies
    ADD CONSTRAINT user_media_policies_pkey PRIMARY KEY (user_id);



ALTER TABLE ONLY user_preferences
    ADD CONSTRAINT user_preferences_pkey PRIMARY KEY (user_id);



ALTER TABLE ONLY users
    ADD CONSTRAINT users_pkey PRIMARY KEY (id);



ALTER TABLE ONLY work_feedback
    ADD CONSTRAINT work_feedback_pkey PRIMARY KEY (user_id, work_id);



ALTER TABLE ONLY work_image_data
    ADD CONSTRAINT work_image_data_pkey PRIMARY KEY (image_id, width);



ALTER TABLE ONLY work_image_entries
    ADD CONSTRAINT work_image_entries_pkey PRIMARY KEY (id);



ALTER TABLE ONLY work_image_hidden
    ADD CONSTRAINT work_image_hidden_pkey PRIMARY KEY (work_id, image_id);



ALTER TABLE ONLY work_image_order
    ADD CONSTRAINT work_image_order_pkey PRIMARY KEY (work_id, image_id);



ALTER TABLE ONLY work_posters
    ADD CONSTRAINT work_posters_pkey PRIMARY KEY (work_id);



CREATE INDEX active_playbacks_session ON active_playbacks USING btree (session_hash);



CREATE INDEX active_playbacks_updated ON active_playbacks USING btree (updated_at);



CREATE INDEX api_keys_owner ON api_keys USING btree (user_id);



CREATE INDEX assets_orphaned ON assets USING btree (orphaned_at) WHERE (orphaned_at IS NOT NULL);



CREATE INDEX automation_events_pending ON automation_events USING btree (created_at, id) WHERE (dispatched_at IS NULL);



CREATE INDEX automation_provider_cache_expiry ON automation_provider_cache USING btree (provider, expires_at);



CREATE INDEX automation_sweeps_pending ON automation_sweeps USING btree (created_at, id) WHERE (completed_at IS NULL);



CREATE INDEX automation_tasks_extension ON automation_task_links USING btree (extension_id, task_id);



CREATE INDEX automation_tasks_provider ON automation_task_links USING btree (provider, task_id);



CREATE INDEX durable_tasks_dispatch ON durable_tasks USING btree (node_id, status, priority DESC, created_at, id);



CREATE INDEX durable_tasks_expiry ON durable_tasks USING btree (status, lease_until);



CREATE INDEX durable_tasks_leased ON durable_tasks USING btree (lease_until, id) WHERE (lease_until IS NOT NULL);



CREATE INDEX durable_tasks_leased_library ON durable_tasks USING btree (library_id) WHERE (lease_until IS NOT NULL);



CREATE INDEX durable_tasks_leased_node ON durable_tasks USING btree (node_id) WHERE (lease_until IS NOT NULL);



CREATE INDEX durable_tasks_library ON durable_tasks USING btree (library_id, status);



CREATE INDEX durable_tasks_pending ON durable_tasks USING btree (status) WHERE (status = ANY (ARRAY['queued'::text, 'running'::text, 'paused'::text]));



CREATE INDEX durable_tasks_ready ON durable_tasks USING btree (node_id, available_at) WHERE ((status = 'queued'::text) AND (lease_until IS NULL));



CREATE INDEX durable_tasks_recent ON durable_tasks USING btree (created_at DESC, id);



CREATE INDEX durable_tasks_system_ready ON durable_tasks USING btree (priority DESC, created_at, id) WHERE ((category = 'system'::text) AND (status = 'queued'::text));



CREATE INDEX extension_avatar_images_asset ON extension_avatar_images USING btree (sha256);



CREATE INDEX extension_avatar_images_pack ON extension_avatar_images USING btree (pack_id);



CREATE INDEX extension_data_expiry ON extension_data USING btree (extension_id, expires_at);



CREATE INDEX extension_objects_owner ON extension_objects USING btree (extension_id, scope_kind, scope_id);



CREATE INDEX extension_runs_recent ON extension_runs USING btree (started_at);



CREATE INDEX inventory_assets_source_available ON inventory_assets USING btree (source_id, available);



CREATE INDEX inventory_display_sort ON inventory_works USING btree (kind, display_title_sort, id);



CREATE INDEX inventory_editions_work ON inventory_editions USING btree (work_id);



CREATE INDEX inventory_episodes_season ON inventory_episodes USING btree (season_id, episode_number);



CREATE INDEX inventory_episodes_work ON inventory_episodes USING btree (work_id);



CREATE INDEX inventory_images_library_available ON inventory_images USING btree (library_id, available);



CREATE INDEX inventory_images_work ON inventory_images USING btree (work_id, available, priority, relative_path);



CREATE INDEX inventory_library_display_sort ON inventory_works USING btree (library_id, kind, display_title_sort, id);



CREATE INDEX inventory_seasons_series ON inventory_seasons USING btree (series_id, season_number);



CREATE INDEX inventory_sources_library ON inventory_sources USING btree (library_id);



CREATE INDEX inventory_sources_library_root ON inventory_sources USING btree (library_id, root_snapshot);



CREATE INDEX inventory_sources_version ON inventory_sources USING btree (version_id);



CREATE INDEX inventory_versions_edition ON inventory_versions USING btree (edition_id);



CREATE INDEX inventory_works_added_at ON inventory_works USING btree (added_at, id);



CREATE INDEX inventory_works_avail_sort ON inventory_works USING btree (has_available_asset, display_title_sort, id, library_id, kind);



CREATE INDEX inventory_works_filter_sort ON inventory_works USING btree (kind, has_available_asset, effective_year, max_height, display_title_sort, id, library_id, codecs_csv, languages_csv, genres_csv);



CREATE INDEX inventory_works_fts_sort ON inventory_works USING btree (id, library_id, kind, has_available_asset, display_title_sort, effective_year, max_height, codecs_csv, languages_csv, genres_csv);



CREATE INDEX inventory_works_global_added_sort ON inventory_works USING btree (kind, added_at, display_title_sort, id);



CREATE INDEX inventory_works_global_avail_sort ON inventory_works USING btree (kind, has_available_asset, display_title_sort, id);



CREATE INDEX inventory_works_global_sort ON inventory_works USING btree (kind, display_title_sort, id);



CREATE INDEX inventory_works_global_year_sort ON inventory_works USING btree (kind, effective_year, display_title_sort, id);



CREATE INDEX inventory_works_lib_avail_sort ON inventory_works USING btree (library_id, kind, has_available_asset, display_title_sort, id);



CREATE INDEX inventory_works_lib_display_sort ON inventory_works USING btree (library_id, kind, display_title_sort, id);



CREATE INDEX inventory_works_library_kind ON inventory_works USING btree (library_id, kind);



CREATE INDEX inventory_works_library_sort ON inventory_works USING btree (library_id, title_sort, id);



CREATE INDEX inventory_works_title_sort ON inventory_works USING btree (title_sort, id);



CREATE INDEX library_artwork_asset ON library_artwork USING btree (sha256);



CREATE INDEX library_folders_parent ON library_folders USING btree (parent_id, active);



CREATE INDEX library_permissions_user ON library_permissions USING btree (user_id, library_id);



CREATE INDEX login_throttle_window ON login_throttle USING btree (window_started);



CREATE INDEX media_group_members_group_work ON media_group_members USING btree (group_id, work_id);



CREATE INDEX media_match_candidates_status ON media_match_candidates USING btree (status, score DESC, id);



CREATE INDEX media_merge_history_group ON media_merge_history USING btree (group_id, created_at DESC);



CREATE INDEX media_work_external_ids_lookup ON media_work_external_ids USING btree (kind, provider, external_id, work_id);



CREATE INDEX music_tracks_library ON music_tracks USING btree (library_id, available);



CREATE INDEX oidc_flows_expiry ON oidc_flows USING btree (expires_at);



CREATE INDEX playback_progress_recent ON playback_progress USING btree (user_id, updated_at DESC);



CREATE INDEX playlist_shares_user ON playlist_shares USING btree (user_id, list_id);



CREATE INDEX poster_previews_asset ON poster_previews USING btree (sha256);



CREATE INDEX scan_staging_files_scan_library ON scan_staging_files USING btree (scan_id, library_id);



CREATE INDEX scan_staging_files_scan_reused ON scan_staging_files USING btree (scan_id, reused);



CREATE INDEX scheduled_scans_due ON scheduled_scans USING btree (enabled, next_run_at);



CREATE INDEX sessions_user ON sessions USING btree (user_id);



CREATE INDEX user_avatars_asset ON user_avatars USING btree (sha256);



CREATE INDEX user_group_members_user ON user_group_members USING btree (user_id);



CREATE INDEX work_image_data_asset ON work_image_data USING btree (sha256);



CREATE INDEX work_image_provider_source ON work_image_entries USING btree (work_id, provider, source_url_hash);



CREATE INDEX work_images_owner ON work_image_entries USING btree (work_id, kind, "position", id);



CREATE INDEX work_posters_asset ON work_posters USING btree (sha256);



ALTER TABLE ONLY active_playbacks
    ADD CONSTRAINT active_playbacks_session_hash_fkey FOREIGN KEY (session_hash) REFERENCES sessions(token_hash) ON DELETE CASCADE;



ALTER TABLE ONLY active_scan_runs
    ADD CONSTRAINT active_scan_runs_library_id_fkey FOREIGN KEY (library_id) REFERENCES libraries(id) ON DELETE CASCADE;



ALTER TABLE ONLY active_scan_runs
    ADD CONSTRAINT active_scan_runs_node_id_fkey FOREIGN KEY (node_id) REFERENCES media_nodes(id);



ALTER TABLE ONLY api_key_libraries
    ADD CONSTRAINT api_key_libraries_key_id_fkey FOREIGN KEY (key_id) REFERENCES api_keys(id) ON DELETE CASCADE;



ALTER TABLE ONLY api_key_libraries
    ADD CONSTRAINT api_key_libraries_library_id_fkey FOREIGN KEY (library_id) REFERENCES libraries(id) ON DELETE CASCADE;



ALTER TABLE ONLY api_keys
    ADD CONSTRAINT api_keys_user_id_fkey FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE;



ALTER TABLE ONLY artwork_image_sources
    ADD CONSTRAINT artwork_image_sources_image_id_fkey FOREIGN KEY (image_id) REFERENCES work_image_entries(id) ON DELETE CASCADE;



ALTER TABLE ONLY artwork_image_sources
    ADD CONSTRAINT artwork_image_sources_work_id_fkey FOREIGN KEY (work_id) REFERENCES inventory_works(id) ON DELETE CASCADE;



ALTER TABLE ONLY automatic_metadata_provenance
    ADD CONSTRAINT automatic_metadata_provenance_extension_id_fkey FOREIGN KEY (extension_id) REFERENCES extensions(id) ON DELETE CASCADE;



ALTER TABLE ONLY automatic_timeline_markers
    ADD CONSTRAINT automatic_timeline_markers_asset_id_fkey FOREIGN KEY (asset_id) REFERENCES inventory_assets(id) ON DELETE CASCADE;



ALTER TABLE ONLY automatic_timeline_markers
    ADD CONSTRAINT automatic_timeline_markers_extension_id_fkey FOREIGN KEY (extension_id) REFERENCES extensions(id) ON DELETE CASCADE;



ALTER TABLE ONLY automatic_timeline_markers
    ADD CONSTRAINT automatic_timeline_markers_task_id_fkey FOREIGN KEY (task_id) REFERENCES durable_tasks(id);



ALTER TABLE ONLY automation_dependencies
    ADD CONSTRAINT automation_dependencies_prerequisite_id_fkey FOREIGN KEY (prerequisite_id) REFERENCES durable_tasks(id) ON DELETE CASCADE;



ALTER TABLE ONLY automation_dependencies
    ADD CONSTRAINT automation_dependencies_task_id_fkey FOREIGN KEY (task_id) REFERENCES durable_tasks(id) ON DELETE CASCADE;



ALTER TABLE ONLY automation_events
    ADD CONSTRAINT automation_events_library_id_fkey FOREIGN KEY (library_id) REFERENCES libraries(id) ON DELETE CASCADE;



ALTER TABLE ONLY automation_observations
    ADD CONSTRAINT automation_observations_library_id_fkey FOREIGN KEY (library_id) REFERENCES libraries(id) ON DELETE CASCADE;



ALTER TABLE ONLY automation_policies
    ADD CONSTRAINT automation_policies_extension_id_fkey FOREIGN KEY (extension_id) REFERENCES extensions(id) ON DELETE CASCADE;



ALTER TABLE ONLY automation_policies
    ADD CONSTRAINT automation_policies_library_id_fkey FOREIGN KEY (library_id) REFERENCES libraries(id) ON DELETE CASCADE;



ALTER TABLE ONLY automation_results
    ADD CONSTRAINT automation_results_task_id_fkey FOREIGN KEY (task_id) REFERENCES durable_tasks(id);



ALTER TABLE ONLY automation_sweeps
    ADD CONSTRAINT automation_sweeps_library_id_fkey FOREIGN KEY (library_id) REFERENCES libraries(id) ON DELETE CASCADE;



ALTER TABLE ONLY automation_task_links
    ADD CONSTRAINT automation_task_links_event_id_fkey FOREIGN KEY (event_id) REFERENCES automation_events(id);



ALTER TABLE ONLY automation_task_links
    ADD CONSTRAINT automation_task_links_task_id_fkey FOREIGN KEY (task_id) REFERENCES durable_tasks(id) ON DELETE CASCADE;



ALTER TABLE ONLY durable_tasks
    ADD CONSTRAINT durable_tasks_library_id_fkey FOREIGN KEY (library_id) REFERENCES libraries(id) ON DELETE CASCADE;



ALTER TABLE ONLY durable_tasks
    ADD CONSTRAINT durable_tasks_node_id_fkey FOREIGN KEY (node_id) REFERENCES media_nodes(id);



ALTER TABLE ONLY extension_avatar_images
    ADD CONSTRAINT extension_avatar_images_pack_id_fkey FOREIGN KEY (pack_id) REFERENCES extension_avatar_packs(id) ON DELETE CASCADE;



ALTER TABLE ONLY extension_avatar_images
    ADD CONSTRAINT extension_avatar_images_sha256_fkey FOREIGN KEY (sha256) REFERENCES assets(sha256);



ALTER TABLE ONLY extension_avatar_packs
    ADD CONSTRAINT extension_avatar_packs_extension_id_fkey FOREIGN KEY (extension_id) REFERENCES extensions(id) ON DELETE CASCADE;



ALTER TABLE ONLY extension_data
    ADD CONSTRAINT extension_data_extension_id_fkey FOREIGN KEY (extension_id) REFERENCES extension_data_namespaces(extension_id);



ALTER TABLE ONLY extension_grants
    ADD CONSTRAINT extension_grants_extension_id_fkey FOREIGN KEY (extension_id) REFERENCES extensions(id) ON DELETE CASCADE;



ALTER TABLE ONLY extension_grants
    ADD CONSTRAINT extension_grants_library_id_fkey FOREIGN KEY (library_id) REFERENCES libraries(id) ON DELETE CASCADE;



ALTER TABLE ONLY extension_object_chunks
    ADD CONSTRAINT extension_object_chunks_object_id_fkey FOREIGN KEY (object_id) REFERENCES extension_objects(id) ON DELETE CASCADE;



ALTER TABLE ONLY extension_settings
    ADD CONSTRAINT extension_settings_extension_id_fkey FOREIGN KEY (extension_id) REFERENCES extensions(id) ON DELETE CASCADE;



ALTER TABLE ONLY extensions
    ADD CONSTRAINT extensions_publisher_id_fkey FOREIGN KEY (publisher_id) REFERENCES extension_publishers(id);



ALTER TABLE ONLY inventory_assets
    ADD CONSTRAINT inventory_assets_library_id_fkey FOREIGN KEY (library_id) REFERENCES libraries(id);



ALTER TABLE ONLY inventory_assets
    ADD CONSTRAINT inventory_assets_source_id_fkey FOREIGN KEY (source_id) REFERENCES inventory_sources(id);



ALTER TABLE ONLY inventory_audio
    ADD CONSTRAINT inventory_audio_asset_id_fkey FOREIGN KEY (asset_id) REFERENCES inventory_assets(id) ON DELETE CASCADE;



ALTER TABLE ONLY inventory_edition_metadata
    ADD CONSTRAINT inventory_edition_metadata_edition_id_fkey FOREIGN KEY (edition_id) REFERENCES inventory_editions(id) ON DELETE CASCADE;



ALTER TABLE ONLY inventory_editions
    ADD CONSTRAINT inventory_editions_work_id_fkey FOREIGN KEY (work_id) REFERENCES inventory_works(id);



ALTER TABLE ONLY inventory_editorial
    ADD CONSTRAINT inventory_editorial_asset_id_fkey FOREIGN KEY (asset_id) REFERENCES inventory_assets(id);



ALTER TABLE ONLY inventory_episodes
    ADD CONSTRAINT inventory_episodes_season_id_fkey FOREIGN KEY (season_id) REFERENCES inventory_seasons(id) ON DELETE CASCADE;



ALTER TABLE ONLY inventory_episodes
    ADD CONSTRAINT inventory_episodes_work_id_fkey FOREIGN KEY (work_id) REFERENCES inventory_works(id) ON DELETE CASCADE;



ALTER TABLE ONLY inventory_images
    ADD CONSTRAINT inventory_images_library_id_fkey FOREIGN KEY (library_id) REFERENCES libraries(id);



ALTER TABLE ONLY inventory_images
    ADD CONSTRAINT inventory_images_work_id_fkey FOREIGN KEY (work_id) REFERENCES inventory_works(id);



ALTER TABLE ONLY inventory_seasons
    ADD CONSTRAINT inventory_seasons_series_id_fkey FOREIGN KEY (series_id) REFERENCES inventory_works(id) ON DELETE CASCADE;



ALTER TABLE ONLY inventory_sources
    ADD CONSTRAINT inventory_sources_library_id_fkey FOREIGN KEY (library_id) REFERENCES libraries(id);



ALTER TABLE ONLY inventory_sources
    ADD CONSTRAINT inventory_sources_version_id_fkey FOREIGN KEY (version_id) REFERENCES inventory_versions(id);



ALTER TABLE ONLY inventory_subtitles
    ADD CONSTRAINT inventory_subtitles_asset_id_fkey FOREIGN KEY (asset_id) REFERENCES inventory_assets(id) ON DELETE CASCADE;



ALTER TABLE ONLY inventory_tracks
    ADD CONSTRAINT inventory_tracks_asset_id_fkey FOREIGN KEY (asset_id) REFERENCES inventory_assets(id);



ALTER TABLE ONLY inventory_version_metadata
    ADD CONSTRAINT inventory_version_metadata_version_id_fkey FOREIGN KEY (version_id) REFERENCES inventory_versions(id) ON DELETE CASCADE;



ALTER TABLE ONLY inventory_versions
    ADD CONSTRAINT inventory_versions_edition_id_fkey FOREIGN KEY (edition_id) REFERENCES inventory_editions(id);



ALTER TABLE ONLY inventory_works
    ADD CONSTRAINT inventory_works_library_id_fkey FOREIGN KEY (library_id) REFERENCES libraries(id);



ALTER TABLE ONLY libraries
    ADD CONSTRAINT libraries_node_id_fkey FOREIGN KEY (node_id) REFERENCES media_nodes(id);



ALTER TABLE ONLY library_artwork
    ADD CONSTRAINT library_artwork_library_id_fkey FOREIGN KEY (library_id) REFERENCES libraries(id) ON DELETE CASCADE;



ALTER TABLE ONLY library_artwork
    ADD CONSTRAINT library_artwork_sha256_fkey FOREIGN KEY (sha256) REFERENCES assets(sha256);



ALTER TABLE ONLY library_folders
    ADD CONSTRAINT library_folders_library_id_fkey FOREIGN KEY (library_id) REFERENCES libraries(id) ON DELETE CASCADE;



ALTER TABLE ONLY library_folders
    ADD CONSTRAINT library_folders_parent_id_fkey FOREIGN KEY (parent_id) REFERENCES libraries(id) ON DELETE CASCADE;



ALTER TABLE ONLY library_permissions
    ADD CONSTRAINT library_permissions_library_id_fkey FOREIGN KEY (library_id) REFERENCES libraries(id) ON DELETE CASCADE;



ALTER TABLE ONLY library_permissions
    ADD CONSTRAINT library_permissions_user_id_fkey FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE;



ALTER TABLE ONLY local_credentials
    ADD CONSTRAINT local_credentials_user_id_fkey FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE;



ALTER TABLE ONLY manual_timeline_markers
    ADD CONSTRAINT manual_timeline_markers_asset_id_fkey FOREIGN KEY (asset_id) REFERENCES inventory_assets(id) ON DELETE CASCADE;



ALTER TABLE ONLY manual_timeline_markers
    ADD CONSTRAINT manual_timeline_markers_updated_by_fkey FOREIGN KEY (updated_by) REFERENCES users(id);



ALTER TABLE ONLY media_group_members
    ADD CONSTRAINT media_group_members_group_id_fkey FOREIGN KEY (group_id) REFERENCES media_groups(id) ON DELETE CASCADE;



ALTER TABLE ONLY media_group_members
    ADD CONSTRAINT media_group_members_work_id_fkey FOREIGN KEY (work_id) REFERENCES inventory_works(id) ON DELETE CASCADE;



ALTER TABLE ONLY media_match_candidates
    ADD CONSTRAINT media_match_candidates_left_work_id_fkey FOREIGN KEY (left_work_id) REFERENCES inventory_works(id) ON DELETE CASCADE;



ALTER TABLE ONLY media_match_candidates
    ADD CONSTRAINT media_match_candidates_right_work_id_fkey FOREIGN KEY (right_work_id) REFERENCES inventory_works(id) ON DELETE CASCADE;



ALTER TABLE ONLY media_work_external_ids
    ADD CONSTRAINT media_work_external_ids_work_id_fkey FOREIGN KEY (work_id) REFERENCES inventory_works(id) ON DELETE CASCADE;



ALTER TABLE ONLY music_scan_runs
    ADD CONSTRAINT music_scan_runs_library_id_fkey FOREIGN KEY (library_id) REFERENCES libraries(id) ON DELETE CASCADE;



ALTER TABLE ONLY music_tracks
    ADD CONSTRAINT music_tracks_library_id_fkey FOREIGN KEY (library_id) REFERENCES libraries(id) ON DELETE CASCADE;



ALTER TABLE ONLY oidc_flows
    ADD CONSTRAINT oidc_flows_link_session_hash_fkey FOREIGN KEY (link_session_hash) REFERENCES sessions(token_hash) ON DELETE CASCADE;



ALTER TABLE ONLY oidc_identities
    ADD CONSTRAINT oidc_identities_user_id_fkey FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE;



ALTER TABLE ONLY parental_blocked_tags
    ADD CONSTRAINT parental_blocked_tags_user_id_fkey FOREIGN KEY (user_id) REFERENCES parental_policies(user_id) ON DELETE CASCADE;



ALTER TABLE ONLY parental_policies
    ADD CONSTRAINT parental_policies_user_id_fkey FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE;



ALTER TABLE ONLY personal_list_items
    ADD CONSTRAINT personal_list_items_list_id_fkey FOREIGN KEY (list_id) REFERENCES personal_lists(id) ON DELETE CASCADE;



ALTER TABLE ONLY personal_list_items
    ADD CONSTRAINT personal_list_items_work_id_fkey FOREIGN KEY (work_id) REFERENCES inventory_works(id) ON DELETE CASCADE;



ALTER TABLE ONLY personal_lists
    ADD CONSTRAINT personal_lists_user_id_fkey FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE;



ALTER TABLE ONLY playback_progress
    ADD CONSTRAINT playback_progress_user_id_fkey FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE;



ALTER TABLE ONLY playback_progress
    ADD CONSTRAINT playback_progress_work_id_fkey FOREIGN KEY (work_id) REFERENCES inventory_works(id) ON DELETE CASCADE;



ALTER TABLE ONLY playlist_music_items
    ADD CONSTRAINT playlist_music_items_list_id_fkey FOREIGN KEY (list_id) REFERENCES personal_lists(id) ON DELETE CASCADE;



ALTER TABLE ONLY playlist_music_items
    ADD CONSTRAINT playlist_music_items_track_id_fkey FOREIGN KEY (track_id) REFERENCES music_tracks(id) ON DELETE CASCADE;



ALTER TABLE ONLY playlist_shares
    ADD CONSTRAINT playlist_shares_list_id_fkey FOREIGN KEY (list_id) REFERENCES personal_lists(id) ON DELETE CASCADE;



ALTER TABLE ONLY playlist_shares
    ADD CONSTRAINT playlist_shares_user_id_fkey FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE;



ALTER TABLE ONLY poster_previews
    ADD CONSTRAINT poster_previews_sha256_fkey FOREIGN KEY (sha256) REFERENCES assets(sha256);



ALTER TABLE ONLY poster_previews
    ADD CONSTRAINT poster_previews_work_id_fkey FOREIGN KEY (work_id) REFERENCES work_posters(work_id) ON DELETE CASCADE;



ALTER TABLE ONLY scan_jobs
    ADD CONSTRAINT scan_jobs_library_id_fkey FOREIGN KEY (library_id) REFERENCES libraries(id);



ALTER TABLE ONLY scheduled_scans
    ADD CONSTRAINT scheduled_scans_library_id_fkey FOREIGN KEY (library_id) REFERENCES libraries(id) ON DELETE CASCADE;



ALTER TABLE ONLY sessions
    ADD CONSTRAINT sessions_user_id_fkey FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE;



ALTER TABLE ONLY user_avatars
    ADD CONSTRAINT user_avatars_sha256_fkey FOREIGN KEY (sha256) REFERENCES assets(sha256);



ALTER TABLE ONLY user_avatars
    ADD CONSTRAINT user_avatars_user_id_fkey FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE;



ALTER TABLE ONLY user_group_libraries
    ADD CONSTRAINT user_group_libraries_group_id_fkey FOREIGN KEY (group_id) REFERENCES user_groups(id) ON DELETE CASCADE;



ALTER TABLE ONLY user_group_libraries
    ADD CONSTRAINT user_group_libraries_library_id_fkey FOREIGN KEY (library_id) REFERENCES libraries(id) ON DELETE CASCADE;



ALTER TABLE ONLY user_group_members
    ADD CONSTRAINT user_group_members_group_id_fkey FOREIGN KEY (group_id) REFERENCES user_groups(id) ON DELETE CASCADE;



ALTER TABLE ONLY user_group_members
    ADD CONSTRAINT user_group_members_user_id_fkey FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE;



ALTER TABLE ONLY user_media_policies
    ADD CONSTRAINT user_media_policies_user_id_fkey FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE;



ALTER TABLE ONLY user_preferences
    ADD CONSTRAINT user_preferences_user_id_fkey FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE;



ALTER TABLE ONLY work_feedback
    ADD CONSTRAINT work_feedback_user_id_fkey FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE;



ALTER TABLE ONLY work_feedback
    ADD CONSTRAINT work_feedback_work_id_fkey FOREIGN KEY (work_id) REFERENCES inventory_works(id) ON DELETE CASCADE;



ALTER TABLE ONLY work_image_data
    ADD CONSTRAINT work_image_data_image_id_fkey FOREIGN KEY (image_id) REFERENCES work_image_entries(id) ON DELETE CASCADE;



ALTER TABLE ONLY work_image_data
    ADD CONSTRAINT work_image_data_sha256_fkey FOREIGN KEY (sha256) REFERENCES assets(sha256);



ALTER TABLE ONLY work_image_entries
    ADD CONSTRAINT work_image_entries_work_id_fkey FOREIGN KEY (work_id) REFERENCES inventory_works(id) ON DELETE CASCADE;



ALTER TABLE ONLY work_image_hidden
    ADD CONSTRAINT work_image_hidden_work_id_fkey FOREIGN KEY (work_id) REFERENCES inventory_works(id) ON DELETE CASCADE;



ALTER TABLE ONLY work_image_order
    ADD CONSTRAINT work_image_order_work_id_fkey FOREIGN KEY (work_id) REFERENCES inventory_works(id) ON DELETE CASCADE;



ALTER TABLE ONLY work_posters
    ADD CONSTRAINT work_posters_sha256_fkey FOREIGN KEY (sha256) REFERENCES assets(sha256);



ALTER TABLE ONLY work_posters
    ADD CONSTRAINT work_posters_work_id_fkey FOREIGN KEY (work_id) REFERENCES inventory_works(id) ON DELETE CASCADE;

CREATE TABLE watch_parties (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    host_user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    media_id TEXT NOT NULL,
    control_mode TEXT NOT NULL DEFAULT 'host_only' CHECK(control_mode IN ('host_only', 'shared', 'request')),
    state TEXT NOT NULL DEFAULT 'waiting' CHECK(state IN ('waiting', 'playing', 'paused', 'ended')),
    current_position_ms BIGINT NOT NULL DEFAULT 0,
    position_updated_at_ms BIGINT NOT NULL DEFAULT 0,
    is_locked BIGINT NOT NULL DEFAULT 0 CHECK(is_locked IN (0, 1)),
    wait_for_buffering BIGINT NOT NULL DEFAULT 1 CHECK(wait_for_buffering IN (0, 1)),
    created_at BIGINT NOT NULL DEFAULT (EXTRACT(EPOCH FROM NOW())::BIGINT),
    ended_at BIGINT
);

CREATE TABLE watch_party_members (
    party_id TEXT NOT NULL REFERENCES watch_parties(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role TEXT NOT NULL DEFAULT 'participant' CHECK(role IN ('host', 'participant')),
    is_ready BIGINT NOT NULL DEFAULT 0 CHECK(is_ready IN (0, 1)),
    is_buffering BIGINT NOT NULL DEFAULT 0 CHECK(is_buffering IN (0, 1)),
    joined_at BIGINT NOT NULL DEFAULT (EXTRACT(EPOCH FROM NOW())::BIGINT),
    last_seen_at BIGINT NOT NULL DEFAULT (EXTRACT(EPOCH FROM NOW())::BIGINT),
    PRIMARY KEY(party_id, user_id)
);

CREATE TABLE watch_party_invites (
    id TEXT PRIMARY KEY NOT NULL,
    party_id TEXT NOT NULL REFERENCES watch_parties(id) ON DELETE CASCADE,
    token TEXT NOT NULL UNIQUE,
    created_by TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    target_user_id TEXT REFERENCES users(id) ON DELETE CASCADE,
    created_at BIGINT NOT NULL DEFAULT (EXTRACT(EPOCH FROM NOW())::BIGINT),
    expires_at BIGINT NOT NULL,
    revoked_at BIGINT
);

CREATE TABLE watch_party_messages (
    id TEXT PRIMARY KEY NOT NULL,
    party_id TEXT NOT NULL REFERENCES watch_parties(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    content TEXT NOT NULL CHECK(length(content) BETWEEN 1 AND 500),
    kind TEXT NOT NULL DEFAULT 'chat' CHECK(kind IN ('chat', 'system', 'reaction')),
    created_at BIGINT NOT NULL DEFAULT (EXTRACT(EPOCH FROM NOW())::BIGINT)
);

CREATE TABLE watch_party_queue (
    id TEXT PRIMARY KEY NOT NULL,
    party_id TEXT NOT NULL REFERENCES watch_parties(id) ON DELETE CASCADE,
    media_id TEXT NOT NULL,
    added_by TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    position BIGINT NOT NULL DEFAULT 0,
    created_at BIGINT NOT NULL DEFAULT (EXTRACT(EPOCH FROM NOW())::BIGINT)
);

CREATE TABLE watch_party_polls (
    id TEXT PRIMARY KEY NOT NULL,
    party_id TEXT NOT NULL REFERENCES watch_parties(id) ON DELETE CASCADE,
    question TEXT NOT NULL CHECK(length(question) BETWEEN 1 AND 200),
    options_json TEXT NOT NULL,
    created_by TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    is_closed BIGINT NOT NULL DEFAULT 0 CHECK(is_closed IN (0, 1)),
    created_at BIGINT NOT NULL DEFAULT (EXTRACT(EPOCH FROM NOW())::BIGINT)
);

CREATE TABLE watch_party_poll_votes (
    poll_id TEXT NOT NULL REFERENCES watch_party_polls(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    option_index BIGINT NOT NULL,
    PRIMARY KEY(poll_id, user_id)
);

CREATE TABLE schema_version(version INTEGER PRIMARY KEY CHECK(version=1));
INSERT INTO schema_version(version) VALUES(1);

-- Initial rows
INSERT INTO media_nodes (id, name, kind, status, key_id, shared_secret, roots_json, paired_at, connection_mode)
VALUES ('local', 'Node local (Standalone)', 'local', 'connected', 'local', '', '[]', (EXTRACT(EPOCH FROM NOW())::BIGINT), 'outbound');

INSERT INTO backup_schedule (singleton, enabled, interval_minutes, next_due_at, last_outcome)
VALUES (1, 0, 1440, 0, 'never');

INSERT INTO playlist_policy (singleton, allow_public, allow_sharing)
VALUES (1, 0, 0);

INSERT INTO inventory_cache_generation (id, gen)
VALUES (1, 1);

INSERT INTO extension_repositories (id, name, url, manifest_json, refreshed_at)
VALUES ('community-default', 'Aether Plugins', 'https://raw.githubusercontent.com/maelmoreau21/Aether-Plugins/main/repository/manifest.json', '{"apiVersion":1,"extensions":[]}', 0);

INSERT INTO system_task_schedules (id, name, description, category, interval_minutes, enabled, next_due_at) VALUES
('update-extensions', 'Mettre à jour les plugins', 'Vérifie les dépôts distants et actualise les catalogues d''extensions.', 'Extensions', 1440, 1, (EXTRACT(EPOCH FROM NOW())::BIGINT) + 86400),
('optimize-database', 'Optimiser la base de données', 'Exécute l''optimisation du moteur de base de données et des journaux de transactions.', 'Maintenance', 1440, 1, (EXTRACT(EPOCH FROM NOW())::BIGINT) + 86400),
('clean-sessions', 'Nettoyer les sessions expirées', 'Supprime les sessions obsolètes et les flux d''authentification expirés.', 'Maintenance', 360, 1, (EXTRACT(EPOCH FROM NOW())::BIGINT) + 21600),
('clean-activity-log', 'Nettoyer le journal des extensions', 'Supprime au plus 500 événements du journal des extensions antérieurs à la rétention configurée.', 'Maintenance', 1440, 1, (EXTRACT(EPOCH FROM NOW())::BIGINT) + 86400),
('clean-cache', 'Nettoyer le cache et fichiers temporaires', 'Supprime les fichiers temporaires périmés des dossiers appartenant à Aether (jamais les médias).', 'Maintenance', 1440, 1, (EXTRACT(EPOCH FROM NOW())::BIGINT) + 86400),
('clean-playlists', 'Nettoyer les listes et collections', 'Retire des listes de lecture personnelles les éléments dont les médias ont été supprimés.', 'Maintenance', 1440, 1, (EXTRACT(EPOCH FROM NOW())::BIGINT) + 86400);

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
-- Views effective_user_group_members and effective_library_permissions: derived.sql.

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
