CREATE TABLE oidc_identities (
    issuer TEXT NOT NULL,
    subject TEXT NOT NULL CHECK(length(subject) BETWEEN 1 AND 255),
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    PRIMARY KEY(issuer, subject),
    UNIQUE(issuer, user_id)
);
CREATE TABLE oidc_flows (
    state_hash TEXT PRIMARY KEY NOT NULL,
    browser_hash TEXT NOT NULL,
    nonce TEXT NOT NULL,
    verifier TEXT NOT NULL,
    link_session_hash TEXT REFERENCES sessions(token_hash) ON DELETE CASCADE,
    expires_at BIGINT NOT NULL
);
CREATE INDEX oidc_flows_expiry ON oidc_flows(expires_at);
