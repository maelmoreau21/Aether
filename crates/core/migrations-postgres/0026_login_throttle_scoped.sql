-- Scoped IP-based login rate limiting to mitigate DoS and credential stuffing
DROP TABLE login_throttle;
CREATE TABLE login_throttle (
    throttle_key TEXT PRIMARY KEY NOT NULL,
    window_started BIGINT NOT NULL,
    attempts BIGINT NOT NULL
);
CREATE INDEX login_throttle_window ON login_throttle(window_started);
