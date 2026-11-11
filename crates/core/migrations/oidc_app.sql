-- Single sign-on started by a native application in the system browser.
-- A row is the pending request (bound to the OIDC state), then the one-time
-- code handed back to the application, redeemed with its verifier.
CREATE TABLE IF NOT EXISTS oidc_app_grants (
    state_hash TEXT PRIMARY KEY,
    challenge TEXT NOT NULL,
    redirect TEXT NOT NULL,
    code_hash TEXT UNIQUE,
    user_id TEXT REFERENCES users(id) ON DELETE CASCADE,
    expires_at BIGINT NOT NULL
);
CREATE INDEX IF NOT EXISTS oidc_app_grants_expiry ON oidc_app_grants(expires_at);
