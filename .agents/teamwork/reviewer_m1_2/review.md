# Milestone 1 Adversarial Review Report

**Reviewer**: Reviewer 2 (Milestone 1)
**Date**: 2026-10-16T13:12:00Z
**Verdict**: **APPROVE**

---

## 1. Executive Summary

Milestone 1 implements the backend containerization adaptations required for the all-in-one Aether Docker standalone distribution:
1. Flexible network binding via `MEDIA_BIND_ADDR` (defaulting safely to `127.0.0.1`, configurable to `0.0.0.0` for Docker).
2. Configurable static web distribution via `MEDIA_WEB_DIST` (defaulting to `apps/web/build`, canonicalized at startup, gracefully warning if missing).
3. Resilient healthcheck route decoupling: `/api/v1/health` is decoupled from `local_security` so internal Docker health probes (`Host: 127.0.0.1:3000` or `localhost:3000`) succeed with `200 OK` while `MEDIA_PUBLIC_URL` points to an external reverse-proxy domain, while all sensitive routes remain strictly protected.

All verification commands (`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, `cargo test --all-targets --features fixture-bench`) pass with zero errors and zero warnings. No integrity violations or facade implementations were detected.

---

## 2. Verification of Claims & Commands

| Claim | Verification Method | Result | Notes |
|---|---|---|---|
| `cargo fmt --check` clean | Executed in repo root | **PASS** | Exit code 0, 0 formatting issues |
| `cargo clippy --all-targets -- -D warnings` clean | Executed in repo root | **PASS** | Exit code 0, 0 warnings |
| `cargo test` passes | Executed in repo root | **PASS** | Exit code 0, 44 tests passed, 0 failed, 5 ignored (browser/Playwright) |
| `cargo test --all-targets --features fixture-bench` passes | Executed in repo root | **PASS** | Exit code 0, 58 tests passed, 0 failed, 5 ignored |
| Startup fails closed on invalid `MEDIA_BIND_ADDR` | Executed with `MEDIA_BIND_ADDR="invalid-ip"` | **PASS** | Exited with code 1: `invalid MEDIA_BIND_ADDR 'invalid-ip': invalid IPv4 address syntax` |
| Route decoupling protects all non-health routes | `tests/healthcheck_resilience.rs` | **PASS** | Validated: `/api/v1/health` returns 200; `/api/v1/catalog`, `/api/v1/auth/login`, `/api/v1/config`, `/app/`, `/api/v1/assets`, `/api/v1/admin/libraries` return 403 Forbidden under loopback Host mismatch |
| Static asset serving under custom path | `tests/static_web.rs` | **PASS** | Serves custom index, custom subpaths, returns 404 for missing files, enforces security headers and `local_security` |

---

## 3. Adversarial & Security Findings

### Finding 1 [Minor / Informational]: Strict IPv4 Binding Limitation
- **What**: `parse_bind_addr` parses `Ipv4Addr` and binds using `SocketAddrV4::new(bind_addr, port)`. Passing an IPv6 address (such as `::1` or `::`) returns an error and aborts startup.
- **Where**: `crates/core/src/main.rs:7-26`
- **Why**: `PROJECT.md` specifies IPv4: `MEDIA_BIND_ADDR: IPv4 address to listen on (default 127.0.0.1, set to 0.0.0.0 in container)`. The current implementation adheres strictly to this contract and fails closed when given an invalid or unsupported address string.
- **Risk Assessment**: Low / Acceptable for Milestone 1. Fails closed and prevents accidental open sockets.
- **Suggestion**: If dual-stack IPv6 container binding is desired in future milestones, transition `parse_bind_addr` to `std::net::IpAddr` and `SocketAddr`.

### Finding 2 [Minor / Informational]: CWD Drift Resilience on Missing Web Dist
- **What**: When `MEDIA_WEB_DIST` points to a path that does not exist at startup, `raw.canonicalize().unwrap_or(raw)` returns the relative path `apps/web/build`.
- **Where**: `crates/core/src/lib.rs:152-165`
- **Why**: If a process changes its current working directory (CWD) after startup, subsequent requests to `/app` would look relative to the new CWD.
- **Risk Assessment**: Negligible. In production Docker containers, the working directory is static (`WORKDIR /app`), assets are baked into the image at build time, and `canonicalize()` resolves an absolute path at startup.
- **Suggestion**: No changes required for M1; behavior is already resilient and emits a warning on missing web dist.

### Finding 3 [Confirmed Secure]: Route Decoupling & Attack Surface Analysis
- **What**: `/api/v1/health` is merged at the root router level outside `local_security`.
- **Where**: `crates/core/src/lib.rs:119, 145`
- **Analysis**:
  - The healthcheck endpoint exposes only `{"status":"ok", "mode":mode}`.
  - No database, user, session, or internal secret information is leaked.
  - Route matching in Axum is exact; `/api/v1/health/` or subpaths do not match.
  - All sensitive routes (`/app`, `/api/v1/config`, `/api/v1/catalog`, `/api/v1/assets`, `/api/v1/admin`, `/api/v1/auth`, etc.) remain strictly guarded by `local_security`.
  - Security headers `X-Content-Type-Options: nosniff` and `Cache-Control: no-store` are properly attached.
- **Verdict**: Fully approved. Resolves the reverse-proxy / container probe incompatibility without compromising security boundaries.

---

## 4. Integrity Check

- **Hardcoded test data / outputs**: None. Tests dynamically create temporary files, run real SQLite migrations, execute Argon2id password hashing, and send real HTTP requests through the Axum router.
- **Dummy or facade implementations**: None. Both `parse_bind_addr` and `configured_web_dist` implement complete parsing, trimming, error mapping, and canonicalization.
- **Shortcuts / task bypasses**: None. All requirements of Milestone 1 were implemented cleanly.
- **Fabricated verification outputs**: None. All commands and test executions were directly and independently verified.

---

## 5. Conformance Check

- **`AGENTS.md`**:
  - Rule 1 & Rule 3: Single process model preserved; no Node.js runtime process or second server process introduced.
  - Rule 5: Generic naming adhered to (`MEDIA_BIND_ADDR`, `MEDIA_WEB_DIST`, `MEDIA_PUBLIC_URL`, `MEDIA_PORT`).
  - Rule 7: RBAC and zero default admin credentials preserved.
  - Rule 9: Rust formatting, clippy, and test rules satisfied without warnings.
- **`ARCHITECTURE.md`**: Router boundaries and mode designations (`standalone-local`, `localhost-fixtures`) conform to architecture specifications.
- **`docs/SECURITY.md`**: Host, Origin, and CSRF protections remain fully intact for all application and data endpoints.

---

## 6. Verdict

**APPROVE**. Milestone 1 is verified, resilient, and ready for Milestone 2 (`Dockerfile` and `.dockerignore`).
