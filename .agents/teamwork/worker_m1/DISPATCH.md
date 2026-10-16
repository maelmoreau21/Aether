## 2026-10-16T12:43:55Z

You are the Milestone 1 Implementation Worker for the Aether project.
Your working directory is: c:\Users\Mael\Documents\GitHub\Aether\.agents\teamwork\worker_m1\
You must read c:\Users\Mael\Documents\GitHub\Aether\ORIGINAL_REQUEST.md before starting work.
Also read c:\Users\Mael\Documents\GitHub\Aether\.agents\teamwork\orchestrator_1\PROJECT.md.

Read the synthesis reports from the Explorers:
- c:\Users\Mael\Documents\GitHub\Aether\.agents\teamwork\explorer_m1_1\analysis.md
- c:\Users\Mael\Documents\GitHub\Aether\.agents\teamwork\explorer_m1_2\analysis.md
- c:\Users\Mael\Documents\GitHub\Aether\.agents\teamwork\explorer_m1_3\analysis.md

Exclusive Write Ownership:
You have exclusive write ownership of:
- crates/core/src/main.rs
- crates/core/src/lib.rs
- crates/core/tests/static_web.rs
- crates/core/tests/healthcheck_resilience.rs

Task Description:
Implement Milestone 1: Backend Containerization Adaptations.
1. In `crates/core/src/main.rs`:
   - Implement `parse_bind_addr` reading `MEDIA_BIND_ADDR` (defaulting to 127.0.0.1 if unset, empty, or whitespace; parsing valid IPv4; returning descriptive `std::io::Error` on invalid syntax).
   - Bind `tokio::net::TcpListener` using `SocketAddrV4::new(bind_addr, port)`.
   - Update `println!` to reflect `{public_origin}/app/`.
   - Add unit tests for `parse_bind_addr_from` covering all edge cases.
2. In `crates/core/src/lib.rs`:
   - Implement `configured_web_dist() -> PathBuf` reading `MEDIA_WEB_DIST` (defaulting to `"apps/web/build"`, best-effort canonicalization when existing to lock in absolute path without failing if missing).
   - In `public_routes`, use `ServeDir::new(configured_web_dist()).append_index_html_on_directories(true)`.
   - Isolate `/api/v1/health` into a dedicated `health_routes(mode: &'static str) -> Router` decorated with `X-Content-Type-Options: nosniff` and `Cache-Control: no-store`.
   - In `secure_app_with_jobs` and `app`, merge `health_routes(mode)` at the top level with `local_security(...)`.
   - Ensure all other routes (`/app`, `/api/v1/config`, `/api/v1/auth/*`, `/api/v1/catalog/*`, `/api/v1/assets/*`, `/api/v1/admin/*`) remain strictly inside `local_security`.
3. Add comprehensive tests:
   - In `crates/core/tests/static_web.rs`: verify custom `MEDIA_WEB_DIST`, 200 on existing files, 404 on missing files, and security headers.
   - In `crates/core/tests/healthcheck_resilience.rs`: verify that when `MEDIA_PUBLIC_URL` is set to an external domain (e.g. `https://nas.example.invalid`), `GET /api/v1/health` with `Host: 127.0.0.1:3000` succeeds (200 OK), while `GET /api/v1/catalog` or `POST /api/v1/auth/login` with `Host: 127.0.0.1:3000` is rejected with 403 Forbidden.
4. Run verification commands:
   - `cargo fmt --check`
   - `cargo clippy --all-targets -- -D warnings`
   - `cargo test`
   Ensure all tests pass with zero warnings and clean formatting.
