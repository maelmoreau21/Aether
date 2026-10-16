# Milestone 1 Implementation Handoff Report

## 1. Observation
- **`crates/core/src/main.rs:26`**: Previously hardcoded `tokio::net::TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port)).await?` and logged `http://127.0.0.1:{port}/app/`.
- **`crates/core/src/lib.rs:154`**: Previously hardcoded `ServeDir::new("apps/web/build")` and bundled `/api/v1/health` inside `public_routes`, which was wrapped by `local_security`.
- **`crates/core/src/lib.rs:170-188`**: `local_security` rejects any request where `req.headers().get(header::HOST) != host.as_deref()`, causing internal container healthchecks (`Host: 127.0.0.1:3000`) to fail with HTTP 403 Forbidden when `MEDIA_PUBLIC_URL` points to an external reverse proxy domain (e.g. `https://nas.example.invalid`).
- **Executed Commands & Results**:
  - `cargo fmt --check`: Clean formatting across the workspace (exit code 0).
  - `cargo clippy --all-targets -- -D warnings`: 0 warnings, clean compilation (exit code 0).
  - `cargo clippy --all-targets --features fixture-bench -- -D warnings`: 0 warnings, clean compilation (exit code 0).
  - `cargo test`: 44 tests passed, 0 failed, 5 ignored (browser/Playwright tests) (exit code 0).
  - `cargo test --all-targets --features fixture-bench`: 58 tests passed, 0 failed, 5 ignored (exit code 0).
  - Verified tests include:
    - `unittests src\main.rs`: 3 passed (`parse_bind_addr_default_when_unset_or_empty`, `parse_bind_addr_valid_ipv4`, `parse_bind_addr_invalid_ipv4_returns_error`).
    - `unittests src\lib.rs`: 27 passed (including `configured_web_dist_defaults_when_unset_or_empty`, `configured_web_dist_custom_path`, `configured_web_dist_nonexistent_path_falls_back_to_raw`).
    - `tests\static_web.rs`: 1 passed (`static_web_serves_custom_dist_and_applies_security_headers`).
    - `tests\healthcheck_resilience.rs`: 1 passed (`healthcheck_resilient_under_external_public_url_while_protected_routes_fail_closed`).
    - All regression tests: `catalog.rs` (3 tests), `core_restart.rs` (1 test), `identity.rs` (6 tests), `nfo_flow.rs` (1 test), `p0.rs` (11 tests), `permissions.rs` (2 tests), `posters_flow.rs` (1 test), `scanner_flow.rs` (1 test).

## 2. Logic Chain
1. *Network Binding*: By implementing `parse_bind_addr_from` and `parse_bind_addr`, the daemon accepts `MEDIA_BIND_ADDR` (e.g. `0.0.0.0` for Docker containers), while defaulting to `127.0.0.1` (`Ipv4Addr::LOCALHOST`) if the variable is unset, empty, or whitespace-only. Any malformed IP address immediately returns a descriptive `std::io::Error`, preventing silent misconfiguration.
2. *Configurable Web Distribution*: By implementing `configured_web_dist_from` and `configured_web_dist`, the static file directory is read from `MEDIA_WEB_DIST` (defaulting to `"apps/web/build"`). Best-effort canonicalization converts existing paths to canonical absolute paths at startup, ensuring asset serving remains functional regardless of the working directory, while falling back gracefully to uncanonicalized paths if missing without crashing non-web tests.
3. *Healthcheck Route Decoupling*: Isolating `/api/v1/health` into a distinct `health_routes` router decorated with `nosniff` and `no-store` headers and merging it at the top level with `local_security(...)` allows Docker internal health probes (`Host: 127.0.0.1:3000` or `localhost:3000`) to succeed with `200 OK` and payload `{"status":"ok","mode":...}` even when `MEDIA_PUBLIC_URL` is set to an external domain.
4. *Strict Security Invariance*: All other application routes (`/app`, `/api/v1/config`, `/api/v1/auth/*`, `/api/v1/catalog/*`, `/api/v1/assets/*`, `/api/v1/admin/*`) remain strictly encapsulated within `local_security`. Requests with mismatched `Host` or illegal `Origin` headers continue to be rejected with `403 Forbidden`, fully preserving DNS rebinding and CSRF protections.

## 3. Caveats
- No caveats. All changes strictly adhere to the designated write ownership (`crates/core/src/main.rs`, `crates/core/src/lib.rs`, `crates/core/tests/static_web.rs`, `crates/core/tests/healthcheck_resilience.rs`).

## 4. Conclusion
Milestone 1 is fully implemented, verified, and complete. `media-poc` is now adapted for containerized execution with full runtime configuration support for bind address, static asset distribution, and resilient Docker healthcheck probing without compromising security invariants.

## 5. Verification Method
To independently verify the implementation:
1. Check formatting:
   `cargo fmt --check`
2. Check linting:
   `cargo clippy --all-targets -- -D warnings`
3. Execute standard unit and integration tests:
   `cargo test`
4. Execute full suite including fixture-bench tests:
   `cargo test --all-targets --features fixture-bench`
5. Invalidate by setting `MEDIA_BIND_ADDR=invalid-ip` and confirming startup failure:
   `cargo test --bin media-poc`
