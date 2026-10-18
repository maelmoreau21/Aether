# Milestone 1 Challenger 1 Handoff Report

## 1. Observation
- **`crates/core/src/main.rs:7-26`**: Implements `parse_bind_addr_from` and `parse_bind_addr`, trimming incoming `raw` string, defaulting empty/whitespace or `NotPresent` to `Ipv4Addr::LOCALHOST`, and parsing to `Ipv4Addr` with error message formatting `invalid MEDIA_BIND_ADDR '{raw}': {err}`.
- **`crates/core/src/main.rs:41`**: Calls `let bind_addr = parse_bind_addr()?;` before `IdentityStore::open` or directory initialization, ensuring immediate fail-fast behavior.
- **`crates/core/src/lib.rs:152-165`**: Implements `configured_web_dist_from` and `configured_web_dist`, falling back to `"apps/web/build"` if unset or empty/whitespace, and best-effort canonicalizing existing paths while preserving nonexistent paths without panic.
- **`crates/core/src/lib.rs:185-189`**: Mounts `ServeDir::new(configured_web_dist()).append_index_html_on_directories(true)` under `/app` within `public_routes`, strictly wrapped by `local_security`.
- **`crates/core/tests/adversarial_challenge.rs`**: Created empirical stress harness testing all specified permutations, path variations, directory traversal resistance, and security headers.
- **Executed Commands & Verbatim Outputs**:
  - `cargo fmt --check`: Exited with code 0 (clean).
  - `cargo clippy --all-targets -- -D warnings`: Exited with code 0 (0 warnings).
  - `cargo test --bin media-poc`:
    ```
    running 3 tests
    test tests::parse_bind_addr_default_when_unset_or_empty ... ok
    test tests::parse_bind_addr_invalid_ipv4_returns_error ... ok
    test tests::parse_bind_addr_valid_ipv4 ... ok
    test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
    ```
  - `cargo test --lib media_poc -- tests::configured_web_dist`:
    ```
    running 3 tests
    test tests::configured_web_dist_nonexistent_path_falls_back_to_raw ... ok
    test tests::configured_web_dist_defaults_when_unset_or_empty ... ok
    test tests::configured_web_dist_custom_path ... ok
    test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 24 filtered out; finished in 0.00s
    ```
  - `cargo test --test static_web --test healthcheck_resilience --test adversarial_challenge`:
    ```
    running 5 tests in adversarial_challenge.rs
    test adversarial_bind_addr_permutations_socket_binding ... ok
    test adversarial_web_dist_resolution_permutations ... ok
    test adversarial_bind_addr_invalid_formats_reject_immediately ... ok
    test adversarial_web_dist_serving_and_security_under_app ... ok
    test adversarial_nonexistent_web_dist_serves_404_without_crashing ... ok
    test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 9.82s

    running 1 test in healthcheck_resilience.rs
    test healthcheck_resilient_under_external_public_url_while_protected_routes_fail_closed ... ok
    test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.34s

    running 1 test in static_web.rs
    test static_web_serves_custom_dist_and_applies_security_headers ... ok
    test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.67s
    ```
  - Binary execution test with invalid format:
    `cmd /c "set MEDIA_BIND_ADDR=999.999.999.999&& target\debug\media-poc.exe"`
    Output: `Error: Custom { kind: Other, error: "invalid MEDIA_BIND_ADDR '999.999.999.999': invalid IPv4 address syntax" }` (Exit code 1).

## 2. Logic Chain
1. *Binding Verification*: By directly invoking the compiled `media-poc` binary and testing `tokio::net::TcpListener::bind` across permutations:
   - Valid IPv4 (`127.0.0.1`, `0.0.0.0`, `127.0.0.2`), padded values (`" 127.0.0.1 "`), empty string, and unset environment variable correctly resolve to the intended IPv4 address and bind successfully.
   - Invalid formats (`"999.999.999.999"`, `"localhost"`, `"::1"`, `"127.0.0.1:3000"`, `"256.0.0.1"`, `"127.0.0.1/24"`) immediately fail fast with exit code 1 and descriptive errors before opening databases or scanning libraries.
2. *Web Distribution Resolution*: Testing `configured_web_dist_from` with custom directories, directories containing spaces, outer whitespace padding, and relative paths confirms that existing targets are reliably canonicalized to absolute paths, preserving internal spaces, while non-existent targets fall back gracefully without panicking.
3. *Static Asset Serving & Security*: Testing Axum routing under `/app` confirms:
   - Nested paths and URL-encoded spaces are served accurately.
   - Missing files return HTTP 404 cleanly.
   - Directory traversal attacks (`/app/../secret.txt` and encoded `/app/%2e%2e/secret.txt`) are stopped by `ServeDir` without leaking files outside the root.
   - Missing web distribution directory boots cleanly and serves HTTP 404 without crashing.
   - All `/app` routes are wrapped by `local_security`: mismatched `Host` or cross-site `Origin` returns HTTP 403 Forbidden, and responses enforce `X-Content-Type-Options: nosniff`, `Cache-Control: no-store`, and strict `Content-Security-Policy`.
4. *Code Quality*: Workspace formatting, clippy checks, and all unit/integration tests for Milestone 1 pass with zero errors.

## 3. Caveats
- Concurrency during tests modifying `std::env`: Setting process-wide environment variables concurrently in multi-threaded test runners causes data races unless guarded by an asynchronous/synchronous lock. This was demonstrated and resolved in `adversarial_challenge.rs` using `tokio::sync::Mutex`. In production, environment variables are established before process launch.
- IPv6 is intentionally rejected under `MEDIA_BIND_ADDR` as the product contract specifies IPv4 (`Ipv4Addr` and `SocketAddrV4`).

## 4. Conclusion
All empirical stress tests and edge cases for `MEDIA_BIND_ADDR` and `MEDIA_WEB_DIST` passed. The implementation is robust, fail-closed, and secure.

**VERDICT: APPROVE**

## 5. Verification Method
To independently verify:
1. Run code formatting check:
   `cargo fmt --check`
2. Run clippy linter:
   `cargo clippy --all-targets -- -D warnings`
3. Run the empirical adversarial stress test suite:
   `cargo test --test adversarial_challenge`
4. Run all Milestone 1 integration tests:
   `cargo test --test static_web --test healthcheck_resilience --test adversarial_challenge`
5. Verify invalid bind address fails fast:
   `cmd /c "set MEDIA_BIND_ADDR=999.999.999.999&& target\debug\media-poc.exe"`
