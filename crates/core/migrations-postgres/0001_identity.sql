-- Small compatibility functions for the existing parameterized SQL. No extensions
-- or administrator privileges are required; resolution is fixed to pg_catalog.
CREATE FUNCTION unixepoch() RETURNS BIGINT
LANGUAGE sql STABLE SET search_path = pg_catalog
AS $$ SELECT floor(extract(epoch FROM statement_timestamp()))::bigint $$;

CREATE FUNCTION instr(TEXT, TEXT) RETURNS BIGINT
LANGUAGE sql IMMUTABLE STRICT SET search_path = pg_catalog
AS $$ SELECT strpos($1, $2)::bigint $$;

CREATE FUNCTION json_valid(input TEXT) RETURNS BIGINT
LANGUAGE plpgsql IMMUTABLE STRICT SET search_path = pg_catalog
AS $$
BEGIN
    PERFORM input::json;
    RETURN 1;
EXCEPTION WHEN invalid_text_representation THEN
    RETURN 0;
END
$$;

-- Only the $.field path shape used by scan staging is supported. Invalid JSON
-- returns NULL too: PostgreSQL can reorder json_valid and json_extract filters.
CREATE FUNCTION json_extract(input TEXT, path TEXT) RETURNS TEXT
LANGUAGE plpgsql IMMUTABLE STRICT SET search_path = pg_catalog
AS $$
BEGIN
    IF path !~ '^\$\.[A-Za-z_][A-Za-z0-9_]*$' THEN
        RAISE EXCEPTION 'unsupported JSON extraction path' USING ERRCODE = '22023';
    END IF;
    RETURN json_extract_path_text(input::json, substr(path, 3));
EXCEPTION WHEN invalid_text_representation THEN
    RETURN NULL;
END
$$;

CREATE TABLE users (
    id TEXT PRIMARY KEY NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('administrator', 'user')),
    created_at BIGINT NOT NULL DEFAULT ((floor(extract(epoch FROM statement_timestamp()))::bigint)),
    disabled_at BIGINT
);
CREATE TABLE local_credentials (
    user_id TEXT PRIMARY KEY NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    login TEXT NOT NULL UNIQUE CHECK(length(login) BETWEEN 3 AND 64 AND login !~ '[^a-z0-9._-]'),
    password_hash TEXT NOT NULL,
    updated_at BIGINT NOT NULL DEFAULT ((floor(extract(epoch FROM statement_timestamp()))::bigint))
);
-- Permanent completion marker: deleting a user must never reopen bootstrap.
CREATE TABLE bootstrap_state (
    singleton BIGINT PRIMARY KEY CHECK(singleton = 1),
    completed_at BIGINT NOT NULL DEFAULT ((floor(extract(epoch FROM statement_timestamp()))::bigint))
);
