-- Who may request what, what is approved without an administrator, and request quotas.
-- Rules are small JSON documents validated by Core on write (acquisition/policy.rs).
-- A missing row inherits: account rule > group rules > server default > built-in default.
-- Rules grant no role and no library access; they only shape the requests application.
CREATE TABLE IF NOT EXISTS request_default_policy (
 singleton BIGINT PRIMARY KEY CHECK(singleton=1), policy_json TEXT NOT NULL
);
INSERT INTO request_default_policy(singleton,policy_json) VALUES(1,'{}') ON CONFLICT(singleton) DO NOTHING;
CREATE TABLE IF NOT EXISTS request_group_policies (
 group_id TEXT PRIMARY KEY REFERENCES user_groups(id) ON DELETE CASCADE, policy_json TEXT NOT NULL, updated_at BIGINT NOT NULL
);
CREATE TABLE IF NOT EXISTS request_user_policies (
 user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE, policy_json TEXT NOT NULL, updated_at BIGINT NOT NULL
);
-- Quota windows count one account's requests by date.
CREATE INDEX IF NOT EXISTS media_requests_quota ON media_requests(user_id,kind,created_at);
