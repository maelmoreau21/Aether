# Handoff Report: Milestone 1 Network Binding and `MEDIA_BIND_ADDR` Support

## 1. Observation

1. **Current Network Binding in `crates/core/src/main.rs` (lines 25-31)**:
   ```rust
   let listener =
       tokio::net::TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port)).await?;
   let port = listener.local_addr()?.port();
   let public_origin =
       std::env::var("MEDIA_PUBLIC_URL").unwrap_or_else(|_| format!("http://127.0.0.1:{port}"));
   let auth = AuthState::new(database.clone(), public_origin).await?;
   println!("Authenticated local Core: http://127.0.0.1:{port}/app/");
   ```
   `TcpListener::bind` explicitly and unconditionally creates a `SocketAddrV4` using `Ipv4Addr::LOCALHOST` (`127.0.0.1`).

2. **Public Origin Validation in `crates/core/src/auth.rs` (lines 76-80)**:
   ```rust
   if uri.authority().is_none_or(|a| a.as_str().contains('@'))
       || uri.path_and_query().is_some_and(|p| p.as_str() != "/")
       || !(scheme == "https"
           || (scheme == "http" && matches!(host, "127.0.0.1" | "localhost" | "[::1]")))
   {
       return Err(AuthConfigError::Origin);
   }
   ```
   `AuthState::new` rejects any non-loopback HTTP origin. If `MEDIA_PUBLIC_URL` were configured with `0.0.0.0`, startup would fail with `Err(AuthConfigError::Origin)`.

3. **Host and Origin Enforcement in `crates/core/src/lib.rs` (lines 179-187)**:
   ```rust
   if req.headers().get(header::HOST).and_then(|h| h.to_str().ok()) != host.as_deref()
       || req.headers().get("sec-fetch-site").is_some_and(|h| h == "cross-site")
       || req.headers().get(header::ORIGIN).is_some_and(|h| h.as_bytes() != origin.as_bytes())
   {
       return StatusCode::FORBIDDEN.into_response();
   }
   ```
   `Host` and `Origin` headers are validated against `host` derived from `public_origin`, independent of the socket listening IP.

4. **Subprocess Test Execution in `crates/core/tests/core_restart.rs` (lines 24-44)**:
   ```rust
   let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_media-poc"))
       .current_dir(root)
       .env("MEDIA_DB_PATH", db)
       .env("MEDIA_PORT", port.to_string())
       .env("MEDIA_FIXTURE_DIR", root.join("tests/fixtures/generated"))
       .env_remove("MEDIA_PUBLIC_URL")
       .stdout(Stdio::null())
       .stderr(Stdio::null())
       .kill_on_drop(true)
       .spawn()
       .unwrap();
   ...
   if tokio::net::TcpStream::connect(("127.0.0.1", port)).await.is_ok()
   ```
   `core_restart.rs` does not set `MEDIA_BIND_ADDR` and expects the binary to accept TCP connections at `127.0.0.1:{port}`.

5. **Baseline Test Suite Verification**:
   - `cargo test`: 36 passed, 0 failed, 5 ignored (`browser_auth`, `browser_nfo`, `browser_posters`, `browser_scanner`, `bootstrap_process_worker`).
   - `cargo clippy --all-targets -- -D warnings`: Exit code 0, 0 warnings.
   - `cargo fmt --check`: Exit code 0, cleanly formatted.

---

## 2. Logic Chain

1. **Root Cause of Docker Connectivity Failure**:
   - Observation 1 demonstrates that `TcpListener::bind` binds strictly to `Ipv4Addr::LOCALHOST` (`127.0.0.1`).
   - In container networking, Docker port forwarding routes incoming traffic to the container's virtual ethernet interface (`eth0`, e.g. `172.17.0.x`).
   - Sockets bound to `127.0.0.1` reject packets arriving on `eth0` with `ECONNREFUSED`.
   - Therefore, the daemon must listen on `0.0.0.0` (`INADDR_ANY`) when deployed in Docker.

2. **Default Backward Compatibility**:
   - Observation 4 shows that existing automated tests (`core_restart.rs`, browser tests) launch `media-poc` without setting `MEDIA_BIND_ADDR` and connect directly to `127.0.0.1`.
   - If `MEDIA_BIND_ADDR` is unset or empty, defaulting to `127.0.0.1` ensures complete compatibility with all existing workflows and tests without requiring configuration changes.

3. **Security Invariant Preservation**:
   - Observations 2 and 3 show that application security (`Host`, `Origin`, CSRF, session cookies) is anchored to `MEDIA_PUBLIC_URL`, completely decoupled from the transport-level bind address.
   - Binding the socket to `0.0.0.0` inside the container does not permit arbitrary `Host` or `Origin` values; requests must still present headers matching the configured canonical `MEDIA_PUBLIC_URL`.

4. **Error Handling Architecture**:
   - Standard `AddrParseError` produces terse output (`invalid IP address syntax`).
   - `String` does not implement `std::error::Error`, but `std::io::Error` does and implements `Send + Sync`.
   - Parsing via `parse_bind_addr_from` and wrapping errors with `std::io::Error::other(format!("invalid MEDIA_BIND_ADDR '{raw}': {err}"))` provides exact diagnostic feedback and clean integration with `main() -> Result<(), Box<dyn std::error::Error + Send + Sync>>`.

5. **Pure Functional Testability**:
   - By structuring the parser as `parse_bind_addr_from(var: Result<String, std::env::VarError>) -> Result<Ipv4Addr, std::io::Error>`, all test scenarios (unset, empty, whitespace, valid IPv4, padded IPv4, invalid IP, IPv6, port suffix) can be unit-tested without mutating process environment variables.

---

## 3. Caveats

1. **Docker Container Execution**: As this is a read-only investigation phase for Milestone 1, container runtime testing (`docker build` / `docker run`) was not executed. Container packaging belongs to Milestone 2.
2. **Healthcheck Host Bypass**: `MEDIA_BIND_ADDR=0.0.0.0` allows Docker's internal healthcheck to reach `127.0.0.1:3000` inside the container; however, if an external `MEDIA_PUBLIC_URL` is set, `local_security` Host validation on `/api/v1/health` must be handled as designed in Milestone 1 Feature 2.
3. **IPv6 Scope**: In accordance with `PROJECT.md` interface contracts, `MEDIA_BIND_ADDR` is scoped to IPv4. IPv6 addresses are explicitly rejected as invalid formats.

---

## 4. Conclusion

Milestone 1 Network Binding can be implemented safely with zero regressions and clean architecture:
1. Introduce `parse_bind_addr_from` and `parse_bind_addr` in `crates/core/src/main.rs`.
2. Fall back to `Ipv4Addr::LOCALHOST` (`127.0.0.1`) when `MEDIA_BIND_ADDR` is unset, empty, or whitespace-only.
3. Parse and bind to `Ipv4Addr::UNSPECIFIED` (`0.0.0.0`) or custom IPv4 addresses when configured.
4. Fail fast with descriptive `std::io::Error` on invalid IP formats.
5. Update `println!` to reflect `{public_origin}/app/`.
6. Include embedded unit tests in `crates/core/src/main.rs` covering all parse permutations.

The complete diff is documented in `proposed_main.patch` and `analysis.md` in this directory.

---

## 5. Verification Method

1. **Codebase Inspection**:
   - Review proposed changes in `.agents/teamwork/explorer_m1_1/proposed_main.patch` and `.agents/teamwork/explorer_m1_1/analysis.md`.
2. **Unit & Integration Test Execution**:
   - `cargo test --workspace`: Must pass all existing tests (36 tests) and the new `src/main.rs` unit tests (3 tests).
3. **Clippy & Code Formatting**:
   - `cargo clippy --all-targets -- -D warnings`: Must pass with 0 warnings.
   - `cargo fmt --check`: Must pass with clean formatting.
4. **Invalidation Conditions**:
   - If `media-poc` fails to start when `MEDIA_BIND_ADDR` is omitted or empty.
   - If `media-poc` fails to listen on all interfaces when `MEDIA_BIND_ADDR=0.0.0.0`.
   - If `media-poc` accepts invalid IP syntax without logging a clear error message.
