# Milestone 1 Handoff Report — Reviewer 2

## 1. Observation
- **`crates/core/src/main.rs:7-30, 41, 51-63`**: Implemented `parse_bind_addr_from` and `parse_bind_addr` reading `MEDIA_BIND_ADDR`, defaulting to `127.0.0.1` (`Ipv4Addr::LOCALHOST`), and failing closed on invalid IP inputs. Binds `SocketAddrV4::new(bind_addr, port)`. Emits warning if `configured_web_dist()` does not exist on disk.
- **`crates/core/src/lib.rs:119, 145, 152-181, 187`**: Implemented `configured_web_dist_from` and `configured_web_dist` reading `MEDIA_WEB_DIST`, defaulting to `"apps/web/build"`, canonicalizing existing paths. Factored out `health_routes` returning `{"status":"ok", "mode":mode}` with `nosniff` and `no-store` headers, merged outside `local_security`. Left all application routes inside `local_security`.
- **`crates/core/tests/static_web.rs`**: Full integration test verifying custom web distribution directory serving under `/app/`, subpath `/app/login/`, 404 on missing assets, and 403 Forbidden under Host header mismatch.
- **`crates/core/tests/healthcheck_resilience.rs`**: Full integration test verifying `/api/v1/health` succeeds with `Host: 127.0.0.1:3000`, `Host: localhost:3000`, and `Host: nas.example.invalid`, while protected routes (`/api/v1/catalog`, `/api/v1/auth/login`, `/api/v1/config`, `/app/`, `/api/v1/assets`, `/api/v1/admin/libraries`) fail closed (`403 Forbidden`) under Host mismatch.
- **Verification Commands Executed**:
  - `cargo fmt --check`: Exit code 0 (clean formatting).
  - `cargo clippy --all-targets -- -D warnings`: Exit code 0 (0 warnings).
  - `cargo test`: Exit code 0 (44 passed, 0 failed, 5 ignored).
  - `cargo test --all-targets --features fixture-bench`: Exit code 0 (58 passed, 0 failed, 5 ignored).
  - `cargo run --bin media-poc` with `MEDIA_BIND_ADDR="invalid-ip"`: Exited with code 1 (`invalid MEDIA_BIND_ADDR 'invalid-ip': invalid IPv4 address syntax`).

## 2. Logic Chain
1. *Network Flexibility & Safety*: Binding inside a Docker container requires listening on `0.0.0.0`, whereas local self-hosting requires binding on loopback `127.0.0.1`. The implementation provides this flexibility via `MEDIA_BIND_ADDR` while maintaining `127.0.0.1` as the default. Validation rejects malformed inputs and prevents silent misconfiguration.
2. *Path Resolution & Container Assets*: In Docker, assets will reside in `/app/web/build` or custom mounts. `configured_web_dist` trims input, defaults to `"apps/web/build"`, canonicalizes when the path exists, and safely degrades with 404 and a warning if assets are missing, preventing crash loops during partial builds or tests.
3. *Healthcheck Decoupling & Boundary Integrity*: The decoupling of `/api/v1/health` from `local_security` is necessary for container healthcheck commands (`curl`/`wget` to `127.0.0.1`) when `MEDIA_PUBLIC_URL` points to an external reverse-proxy domain. Because `/api/v1/health` returns only static status information without credentials or secrets, and Axum enforces exact path routing, all protected API and static web routes remain strictly defended against DNS rebinding and cross-site requests.
4. *Integrity & Standards Compliance*: No dummy implementations, hardcoded mock responses, or bypasses were found. The code adheres strictly to `AGENTS.md`, `ARCHITECTURE.md`, and `docs/SECURITY.md`.

## 3. Caveats
- `parse_bind_addr` currently supports IPv4 addresses only (`Ipv4Addr`). This matches the `PROJECT.md` specification for Milestone 1. If dual-stack IPv6 container binding is needed in the future, it can be extended to `std::net::IpAddr`.
- No other caveats.

## 4. Conclusion
Milestone 1 is verified and approved without reservations (**APPROVE**). All acceptance criteria and constraints are satisfied. The codebase is ready for Milestone 2 container packaging.

## 5. Verification Method
To independently verify:
```powershell
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo test --all-targets --features fixture-bench
$env:MEDIA_BIND_ADDR="invalid-ip"; cargo run --bin media-poc  # verify fail-closed exit code 1
```
