-- Second factor (TOTP + single-use recovery codes) and passkeys (WebAuthn) for local accounts.
-- Additive and idempotent on SQLite and PostgreSQL. Secrets that only need comparing are hashed;
-- the TOTP seed must stay readable to compute codes, so archives carrying it are encrypted (MDBK0003).
CREATE TABLE IF NOT EXISTS user_totp (
 user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
 secret TEXT NOT NULL,
 confirmed_at BIGINT,
 last_step BIGINT NOT NULL DEFAULT 0,
 created_at BIGINT NOT NULL
);
CREATE TABLE IF NOT EXISTS user_recovery_codes (
 user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
 code_hash TEXT NOT NULL,
 PRIMARY KEY(user_id, code_hash)
);
CREATE TABLE IF NOT EXISTS user_passkeys (
 credential_id TEXT PRIMARY KEY,
 user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
 name TEXT NOT NULL,
 algorithm BIGINT NOT NULL,
 public_key TEXT NOT NULL,
 sign_count BIGINT NOT NULL DEFAULT 0,
 created_at BIGINT NOT NULL,
 last_used_at BIGINT
);
CREATE INDEX IF NOT EXISTS user_passkeys_user ON user_passkeys(user_id);
-- Short-lived, single-use proofs: a password accepted but awaiting its second factor ('mfa'),
-- or a WebAuthn challenge ('passkey_register', 'passkey_login'). Only hashes are stored.
CREATE TABLE IF NOT EXISTS auth_challenges (
 challenge_hash TEXT PRIMARY KEY,
 purpose TEXT NOT NULL,
 user_id TEXT REFERENCES users(id) ON DELETE CASCADE,
 webauthn_hash TEXT,
 bearer BIGINT NOT NULL DEFAULT 0,
 attempts BIGINT NOT NULL DEFAULT 0,
 expires_at BIGINT NOT NULL
);
CREATE INDEX IF NOT EXISTS auth_challenges_expiry ON auth_challenges(expires_at);
